//! The on-screen indicator alliumd draws itself: over the launcher, the in-game menu and Allium's
//! own apps.
//!
//! It looks exactly like the message RetroArch draws for the same key press in a game -- one line
//! of white 2x bitmap text with a black drop shadow at the bottom left, no plate -- because the
//! two are seen seconds apart and should not read as two features. RetroArch's rendering is
//! `sdl_miyoomini_print_msg` in the driver this fork builds; this is the same algorithm over a
//! permissively licensed font (see `osd_font`).

use std::time::Duration;

use anyhow::Result;
use common::display::color::Color;
use common::display::{Display, RectHold};
use common::geom::Rect;
use common::platform::Platform;
use tiny_skia::{PixmapMut, PremultipliedColorU8};
use tokio::time::Instant;

use crate::osd_font::{FIRST_CHAR, GLYPH_HEIGHT, GLYPH_WIDTH, GLYPHS};

/// How long the line stays up after the last change: RetroArch's 180 frames
const HIDE_TIMEOUT: Duration = Duration::from_millis(3000);
/// How often the band is re-flushed when nothing else repaints it. A launcher redraw flushes the
/// whole screen and wipes the band, so this is how long it can stay missing; keep it near one
/// display frame. Deriving it from UI_FRAME_INTERVAL would give 83ms -- half a *launcher* frame at
/// 6fps -- which leaves the band dark for five display frames at a time.
const UI_REDRAW_PERIOD: Duration = Duration::from_millis(16);

/// Screen pixels per font pixel
const SCALE: usize = 2;
/// Screen pixels from one glyph's left edge to the next
const ADVANCE: usize = GLYPH_WIDTH * SCALE;
/// The drop shadow's offset, right and down, in screen pixels
const SHADOW: usize = 2;
/// Screen pixels between the left edge and the first glyph: one RetroArch glyph stride
const LEFT_MARGIN: u32 = 12;
/// Screen pixels between the shadow's last row and the bottom edge
const BOTTOM_MARGIN: u32 = 4;
/// One glyph cell at SCALE plus the rows the shadow overhangs
const BAND_H: u32 = (GLYPH_HEIGHT * SCALE + SHADOW) as u32;
/// Index in GLYPHS of the glyph drawn for characters the font lacks: '?'
const FALLBACK_INDEX: usize = (b'?' - b' ') as usize;

/// The strip the line is drawn in, fixed for the life of the OSD: RetroArch's margins, one glyph
/// cell tall plus the shadow. Full width, because the text changes length while a key repeats and
/// a rect that changed would mean rebuilding the stamper mid-repeat.
///
/// `bottom_inset` is what the app underneath keeps for itself at the bottom of the screen -- the
/// button hint strip the launcher and the in-game menu both draw. RetroArch has no such strip, so
/// its own message sits right at the bottom edge; here the line has to clear the hints or it lands
/// on top of them.
///
/// The bottom of the screen is also the first region the panel scans each frame, which is why the
/// old plate sat in the middle. That mattered when this stamped over RetroArch; RetroArch now draws
/// its own message, and the stamper only serves Allium's own apps, which repaint at ~6fps on input.
fn text_band(width: u32, height: u32, bottom_inset: u32) -> Rect {
    Rect::new(
        LEFT_MARGIN as i32,
        height.saturating_sub(bottom_inset + BOTTOM_MARGIN + BAND_H) as i32,
        width.saturating_sub(2 * LEFT_MARGIN),
        BAND_H,
    )
}

fn glyph(c: char) -> &'static [u8; GLYPH_HEIGHT] {
    let index = u32::from(c).wrapping_sub(u32::from(FIRST_CHAR)) as usize;
    GLYPHS.get(index).unwrap_or(&GLYPHS[FALLBACK_INDEX])
}

/// Paints `text` into `band` the way RetroArch's driver does: each font pixel a white 2x2 block
/// with a black 2x2 shadow two pixels right and down. Whole glyphs only, and only as many as fit
/// with their shadow inside `band`; nothing outside it is written. Draws nothing if `band` does
/// not fit the pixmap -- a panic here would take the daemon down over a cosmetic.
fn draw_text(pixmap: &mut PixmapMut<'_>, band: Rect, text: &str) {
    let (width, height) = (pixmap.width() as i32, pixmap.height() as i32);
    if band.x < 0
        || band.y < 0
        || band.right() > width
        || band.bottom() > height
        || (band.h as usize) < GLYPH_HEIGHT * SCALE + SHADOW
    {
        return;
    }
    let stride = width as usize;
    let (x0, y0) = (band.x as usize, band.y as usize);
    let max_glyphs = (band.w as usize).saturating_sub(SHADOW) / ADVANCE;
    let white: PremultipliedColorU8 = Color::new(0xFF, 0xFF, 0xFF).into();
    let black: PremultipliedColorU8 = Color::new(0, 0, 0).into();
    let pixels = pixmap.pixels_mut();

    // Shadows first, then ink. RetroArch interleaves them per pixel, but a shadow only ever lands
    // down and right of its pixel, on cells whose own ink is drawn later, so ink wins wherever the
    // two meet -- which is exactly what two passes give.
    for (color, dx, dy) in [(black, SHADOW, SHADOW), (white, 0, 0)] {
        for (i, c) in text.chars().take(max_glyphs).enumerate() {
            let gx = x0 + i * ADVANCE + dx;
            let gy = y0 + dy;
            for (row, bits) in glyph(c).iter().enumerate() {
                for col in 0..GLYPH_WIDTH {
                    if (bits & (0x80u8 >> col)) == 0 {
                        continue;
                    }
                    let px = gx + col * SCALE;
                    let py = gy + row * SCALE;
                    for sy in 0..SCALE {
                        let start = (py + sy) * stride + px;
                        pixels[start..start + SCALE].fill(color);
                    }
                }
            }
        }
    }
}

/// How often the band has to be rewritten to stay on screen
enum Refresh {
    /// Nothing repaints under us: re-flush periodically, restore the background on hide
    Periodic { next_redraw: Instant },
    /// The app rewrites the whole frame, so a stamper thread owns the rect
    Continuous(RectHold),
}

/// The framebuffer we draw on, and the strip of it the line occupies
struct Surface<P: Platform> {
    display: P::Display,
    band: Rect,
}

impl<P: Platform> Surface<P> {
    fn new(platform: &mut P, bottom_inset: u32) -> Result<Self> {
        let mut display = platform.display_partial()?;
        let band = text_band(display.width(), display.height(), bottom_inset);
        // Nothing outside the band is ever drawn or restored, so read no more of the frame
        display.read_rect(band)?;
        display.save()?;
        Ok(Self { display, band })
    }

    /// Hands the band to a stamper thread, on platforms that have one. No corner trim: there is no
    /// rounded plate any more, and under a repainting app the whole strip -- the text and the
    /// frame it was snapshotted over -- is repeated for the seconds it is up. Accepted: that path
    /// only serves Allium's own apps now.
    fn hold_band(&mut self) -> Result<Option<RectHold>> {
        self.display.hold_rect(self.band, 0)
    }

    fn flush_band(&mut self) -> Result<()> {
        self.display.flush_rect(self.band)
    }

    /// Repaints a live stamper's pixels in place; false means the hold has to be rebuilt
    fn refresh_band(&mut self, hold: &RectHold) -> Result<bool> {
        self.display.refresh_rect_hold(hold, self.band, 0)
    }

    /// Puts the pre-OSD background back where the band was
    fn restore_band(&mut self) -> Result<()> {
        self.display.load(self.band)?;
        self.display.flush_rect(self.band)
    }

    /// Paints the line over the saved background. `flush` writes the band to the framebuffer,
    /// which is right when nothing else repaints it; under a stamper `refresh_band` publishes and
    /// blits the pixels itself.
    fn draw(&mut self, text: &str, flush: bool) -> Result<()> {
        let band = self.band;
        // Repaint over the saved background so a shorter line leaves no residue
        self.display.load(band)?;
        draw_text(&mut self.display.pixmap_mut(), band, text);
        if flush {
            self.display.flush_rect(band)?;
        }
        Ok(())
    }
}

/// State that exists only while the indicator is on screen
struct Shown<P: Platform> {
    surface: Surface<P>,
    hide_at: Instant,
    refresh: Refresh,
}

/// The one-line indicator drawn over whatever app owns the framebuffer.
pub struct Osd<P: Platform> {
    shown: Option<Shown<P>>,
    bottom_inset: u32,
}

impl<P: Platform> Osd<P> {
    /// `bottom_inset` is the strip the app underneath keeps at the bottom of the screen for its
    /// button hints; see [`text_band`].
    pub fn new(bottom_inset: u32) -> Self {
        Self {
            shown: None,
            bottom_inset,
        }
    }

    /// When `tick()` is next due, or `None` while hidden -- nothing to wake for.
    pub fn next_wake(&self) -> Option<Instant> {
        self.shown.as_ref().map(|shown| match shown.refresh {
            Refresh::Periodic { next_redraw } => shown.hide_at.min(next_redraw),
            Refresh::Continuous(_) => shown.hide_at,
        })
    }

    pub fn show(&mut self, platform: &mut P, text: &str, repainting: bool) -> Result<()> {
        let now = Instant::now();

        // Reuse the surface that is already up whenever the strategy still fits. Rebuilding stops
        // the stamper, spawns a thread and re-mmaps fb0 -- and leaves the band undefended in
        // between, which is what made the old bar flicker while a key autorepeated ~30 times a
        // second.
        if let Some(shown) = self.shown.as_mut() {
            let reusable = matches!(
                (&shown.refresh, repainting),
                (Refresh::Continuous(_), true) | (Refresh::Periodic { .. }, false)
            );
            if reusable {
                let flush = matches!(shown.refresh, Refresh::Periodic { .. });
                shown.surface.draw(text, flush)?;
                let refreshed = match &shown.refresh {
                    Refresh::Continuous(hold) => shown.surface.refresh_band(hold)?,
                    Refresh::Periodic { .. } => true,
                };
                if refreshed {
                    shown.hide_at = now + HIDE_TIMEOUT;
                    if let Refresh::Periodic { next_redraw } = &mut shown.refresh {
                        *next_redraw = now + UI_REDRAW_PERIOD;
                    }
                    return Ok(());
                }
            }
        }

        // Consuming the old state stops its stamper before the new text is drawn
        let mut surface = match self.shown.take() {
            Some(shown) => shown.surface,
            None => Surface::new(platform, self.bottom_inset)?,
        };

        surface.draw(text, !repainting)?;

        let periodic = Refresh::Periodic {
            next_redraw: now + UI_REDRAW_PERIOD,
        };
        self.shown = Some(Shown {
            // Must precede the `surface` move: hold_band takes it by &mut
            refresh: if repainting {
                match surface.hold_band()? {
                    Some(hold) => Refresh::Continuous(hold),
                    None => {
                        // No stamper after all, so nothing has put the band on screen yet
                        surface.flush_band()?;
                        periodic
                    }
                }
            } else {
                periodic
            },
            surface,
            hide_at: now + HIDE_TIMEOUT,
        });
        Ok(())
    }

    pub fn tick(&mut self) -> Result<()> {
        let Some(shown) = self.shown.as_mut() else {
            return Ok(());
        };
        let now = Instant::now();
        if now >= shown.hide_at {
            return self.hide();
        }
        if let Refresh::Periodic { next_redraw } = &mut shown.refresh {
            *next_redraw = now + UI_REDRAW_PERIOD;
            shown.surface.flush_band()?;
        }
        Ok(())
    }

    pub fn hide(&mut self) -> Result<()> {
        let Some(mut shown) = self.shown.take() else {
            return Ok(());
        };
        match shown.refresh {
            // The app repaints the frame itself; only static UI needs the background back
            Refresh::Periodic { .. } => shown.surface.restore_band()?,
            Refresh::Continuous(hold) => drop(hold),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use tiny_skia::Pixmap;

    use super::*;

    const WIDTH: u32 = 640;
    const HEIGHT: u32 = 480;
    /// What the launcher's button hints reserve with the default theme
    const INSET: u32 = 44;

    fn premultiplied(r: u8, g: u8, b: u8) -> PremultipliedColorU8 {
        Color::new(r, g, b).into()
    }

    fn rendered(text: &str, background: PremultipliedColorU8) -> Pixmap {
        let mut pixmap = Pixmap::new(WIDTH, HEIGHT).expect("pixmap");
        pixmap.pixels_mut().fill(background);
        draw_text(&mut pixmap.as_mut(), text_band(WIDTH, HEIGHT, INSET), text);
        pixmap
    }

    fn inside(band: Rect, x: u32, y: u32) -> bool {
        (band.x..band.right()).contains(&(x as i32))
            && (band.y..band.bottom()).contains(&(y as i32))
    }

    #[test]
    fn band_sits_above_whatever_the_app_reserves() {
        // Right at the bottom edge when nothing is reserved, as RetroArch draws it
        let flush = text_band(WIDTH, HEIGHT, 0);
        assert_eq!((flush.x, flush.y, flush.w, flush.h), (12, 450, 616, 26));
        // ...and clear of the button hints when there are any
        let band = text_band(WIDTH, HEIGHT, INSET);
        assert_eq!((band.x, band.y, band.w, band.h), (12, 406, 616, 26));
        assert!(
            band.bottom() <= (HEIGHT - INSET) as i32,
            "the line would land on the button hints"
        );
    }

    #[test]
    fn ink_stays_inside_the_band_and_the_text_extent() {
        let background = premultiplied(10, 20, 30);
        let white = premultiplied(0xFF, 0xFF, 0xFF);
        let black = premultiplied(0, 0, 0);
        let text = "Volume  ############--------  60%";
        let band = text_band(WIDTH, HEIGHT, INSET);
        let pixmap = rendered(text, background);
        let ink_right = band.x + (text.chars().count() * ADVANCE) as i32;
        let mut whites = 0;
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let pixel = pixmap.pixel(x, y).expect("in bounds");
                if !inside(band, x, y) {
                    assert_eq!(pixel, background, "({x}, {y}) outside the band was written");
                } else if pixel == white {
                    whites += 1;
                    assert!((x as i32) < ink_right, "ink at x={x} beyond {ink_right}");
                } else if pixel == black {
                    assert!((x as i32) < ink_right + SHADOW as i32, "shadow at x={x}");
                } else {
                    assert_eq!(pixel, background, "unexpected colour at ({x}, {y})");
                }
            }
        }
        assert!(whites > 0, "nothing drawn");
    }

    #[test]
    fn clips_a_line_longer_than_the_band() {
        let background = premultiplied(10, 20, 30);
        let white = premultiplied(0xFF, 0xFF, 0xFF);
        let band = text_band(WIDTH, HEIGHT, INSET);
        let pixmap = rendered(&"#".repeat(200), background);
        let mut whites = 0;
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let pixel = pixmap.pixel(x, y).expect("in bounds");
                if !inside(band, x, y) {
                    assert_eq!(pixel, background, "({x}, {y}) outside the band was written");
                } else if pixel == white {
                    whites += 1;
                }
            }
        }
        assert!(whites > 0, "nothing drawn");
    }

    #[test]
    fn unknown_characters_draw_the_fallback_glyph() {
        let background = premultiplied(10, 20, 30);
        assert_eq!(
            rendered("Display: Nuit \u{263e}", background).pixels(),
            rendered("Display: Nuit ?", background).pixels()
        );
    }

    #[test]
    fn a_display_too_small_for_the_band_draws_nothing() {
        let mut pixmap = Pixmap::new(20, 10).expect("pixmap");
        draw_text(&mut pixmap.as_mut(), text_band(20, 10, INSET), "x");
    }

    #[test]
    fn glyph_rows_use_only_the_cell_columns() {
        let outside_cell = 0xFFu8 >> GLYPH_WIDTH;
        assert!(GLYPHS.iter().flatten().all(|row| (row & outside_cell) == 0));
    }
}
