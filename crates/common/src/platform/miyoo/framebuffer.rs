use std::os::fd::AsRawFd;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow, bail};
use framebuffer::Framebuffer;
use log::{debug, info, trace, warn};
use tiny_skia::{Pixmap, PixmapMut, PixmapRef};

use crate::display::color::Color;
use crate::display::{Display, HeldPixels, RectHold};
use crate::geom::Rect;

/// How often the stamper checks whether the app has flipped pages, when it cannot wait on vblank
const STAMP_POLL: Duration = Duration::from_millis(1);
/// Repaint at least this often even if nothing appears to have changed. The app may composite into
/// the visible page without ever moving `yoffset`, in which case a flip is never observed and the
/// plate would be overwritten for good.
const STAMP_FLOOR: Duration = Duration::from_millis(8);
/// Gap between blits while sweeping the window from vblank to the plate's own scanout
const STAMP_SWEEP: Duration = Duration::from_millis(3);
/// How many vblank waits to time before deciding whether the driver really blocks on them
const VBLANK_PROBES: u32 = 4;
/// A vblank wait this short is not waiting on a panel, whatever the ioctl returned
const VBLANK_MIN: Duration = Duration::from_millis(4);
/// ...and one this long is not a frame worth chasing
const VBLANK_MAX: Duration = Duration::from_millis(40);

// The standard fbdev vblank wait. It is not in the vendor's mstarFb.h, whose custom range starts
// at 'F' 0x60, so support is unknown until probed -- but the patched RetroArch exposes a working
// VSync option, which suggests the driver implements it. Waiting on the panel rather than polling
// the app is the only trigger that is correct whether or not the app pans.
nix::ioctl_write_ptr_bad!(
    fb_wait_for_vsync,
    nix::request_code_write!(b'F', 0x20, 4),
    u32
);

/// Blocks until the next vblank. `Err` means the driver has no such ioctl.
fn wait_for_vsync(fd: std::os::fd::RawFd) -> nix::Result<()> {
    let zero: u32 = 0;
    // SAFETY: the driver reads a single u32 by pointer; `zero` outlives the call
    unsafe { fb_wait_for_vsync(fd, &zero) }.map(|_| ())
}

/// How far apart consecutive vblanks are, or `None` if the driver has no such ioctl.
///
/// Timing it rather than checking the return code is the point. A driver that accepts the call and
/// returns straight away is indistinguishable from a working one by `is_ok()` alone, and would turn
/// the stamping loop -- whose only sleep is the wait itself -- into a hot spin, stealing the core
/// the emulator is running on and rewriting the plate while the panel is reading it.
fn measure_vblank(fd: std::os::fd::RawFd) -> Option<Duration> {
    // Discard the first: it starts partway into a frame, so it is a partial period
    wait_for_vsync(fd).ok()?;
    let mut period = Duration::MAX;
    for _ in 0..VBLANK_PROBES {
        let start = Instant::now();
        wait_for_vsync(fd).ok()?;
        // The shortest is the one least padded by scheduling delay
        period = period.min(start.elapsed());
    }
    (VBLANK_MIN..=VBLANK_MAX)
        .contains(&period)
        .then_some(period)
}

pub struct FramebufferDisplay {
    pixmap: Pixmap,
    iface: Framebuffer,
    saved: Vec<Pixmap>,
}

impl FramebufferDisplay {
    pub fn new() -> Result<FramebufferDisplay> {
        let mut display = Self::blank()?;
        let frame = display.bounding_box();
        display.read_frame_rect(frame);
        Ok(display)
    }

    /// A display whose pixmap starts empty; the caller reads what it needs with `read_rect`
    pub fn blank() -> Result<FramebufferDisplay> {
        let iface = Framebuffer::new("/dev/fb0")?;
        trace!(
            "init fb: var_screen_info: {:?}, fix_screen_info: {:?}",
            iface.var_screen_info, iface.fix_screen_info,
        );

        let width = iface.var_screen_info.xres;
        let height = iface.var_screen_info.yres;
        let pixmap = Pixmap::new(width, height)
            .ok_or_else(|| anyhow!("Failed to create pixmap {}x{}", width, height))?;

        Ok(FramebufferDisplay {
            pixmap,
            iface,
            saved: Vec::new(),
        })
    }

    /// Copies `rect` of the visible frame into the pixmap, unrotating it and BGRA to RGBA
    fn read_frame_rect(&mut self, rect: Rect) {
        let width = self.pixmap.width() as usize;
        let height = self.pixmap.height() as usize;
        let bytes_per_pixel = (self.iface.var_screen_info.bits_per_pixel / 8) as usize;
        let xoffset = self.iface.var_screen_info.xoffset as usize;
        let yoffset = self.iface.var_screen_info.yoffset as usize;
        let location = (yoffset * width + xoffset) * bytes_per_pixel;

        let x0 = rect.x.max(0) as usize;
        let y0 = rect.y.max(0) as usize;
        let x1 = (rect.right().max(0) as usize).min(width);
        let y1 = (rect.bottom().max(0) as usize).min(height);

        let frame = self.iface.read_frame();
        let pixels = self.pixmap.pixels_mut();
        for y in y0..y1 {
            // The framebuffer is rotated 180 degrees, so both axes run backwards
            let fb_y = height - 1 - y;
            for x in x0..x1 {
                let fb_x = width - 1 - x;
                let fb_idx = location + (fb_y * width + fb_x) * bytes_per_pixel;
                let color = Color::rgba(
                    frame[fb_idx + 2],
                    frame[fb_idx + 1],
                    frame[fb_idx],
                    frame[fb_idx + 3],
                );
                pixels[y * width + x] = color.into();
            }
        }
    }

    /// Packs `area` into fb-ordered rows, so the stamping thread only does memcpy
    fn stamp(&self, area: Rect, corner_radius: u32) -> Option<Stamp> {
        let width = self.width() as usize;
        let height = self.height() as usize;
        let bytes_per_pixel = (self.iface.var_screen_info.bits_per_pixel / 8) as usize;

        let x0 = area.x.max(0) as usize;
        let y0 = area.y.max(0) as usize;
        let x1 = (area.right().max(0) as usize).min(width);
        let y1 = (area.bottom().max(0) as usize).min(height);
        if x0 >= x1 || y0 >= y1 {
            return None;
        }

        // Trim to the rounding so the corners keep the app's pixels, not a frozen frame
        let radius = (corner_radius as usize)
            .min((x1 - x0) / 2)
            .min((y1 - y0) / 2) as f32;
        let row_h = (y1 - y0) as f32;
        // Rows are addressed by the hardware stride so a padded scanline can't skew them
        let stride = self.iface.fix_screen_info.line_length as usize;
        let mut rows = Vec::with_capacity(y1 - y0);
        let mut bytes = Vec::with_capacity((y1 - y0) * (x1 - x0) * bytes_per_pixel);
        for y in y0..y1 {
            let dy = (y - y0) as f32 + 0.5;
            let dist = (radius - dy).max(dy - (row_h - radius)).max(0.0);
            let inset = (radius - (radius * radius - dist * dist).sqrt()).ceil() as usize;
            let (rx0, rx1) = (x0 + inset, x1 - inset);
            if rx0 >= rx1 {
                continue;
            }
            let start = bytes.len();
            // Reversed x, and fb_y below, are the two halves of the 180 degree rotation
            for x in (rx0..rx1).rev() {
                let pixel = self.pixmap.pixels()[y * width + x];
                bytes.extend_from_slice(&[pixel.blue(), pixel.green(), pixel.red(), pixel.alpha()]);
            }
            let fb_y = height - 1 - y;
            let fb_x0 = width - rx1;
            rows.push(StampRow {
                offset: fb_y * stride + fb_x0 * bytes_per_pixel,
                start,
                len: bytes.len() - start,
            });
        }
        if rows.is_empty() {
            return None;
        }

        let pages = (self.iface.var_screen_info.yres_virtual as usize / height.max(1)).clamp(1, 3);
        debug!(
            "holding {}x{} rect over fb {}x{} (virtual {}, stride {}, {} bpp, {} pages)",
            x1 - x0,
            y1 - y0,
            width,
            height,
            self.iface.var_screen_info.yres_virtual,
            self.iface.fix_screen_info.line_length,
            bytes_per_pixel * 8,
            pages,
        );
        Some(Stamp {
            bytes: Arc::new(Mutex::new(bytes.into_boxed_slice())),
            rows: rows.into_boxed_slice(),
            pages,
            page_stride: stride * height,
        })
    }

    fn write_rect(&mut self, rect: Rect) {
        let yoffset = self.iface.var_screen_info.yoffset as usize;
        self.write_rect_at(rect, yoffset);
    }

    fn write_rect_at(&mut self, rect: Rect, yoffset: usize) {
        let xoffset = self.iface.var_screen_info.xoffset as usize;
        let width = self.width() as usize;
        let height = self.height() as usize;
        let bytes_per_pixel = (self.iface.var_screen_info.bits_per_pixel / 8) as usize;
        let location = (yoffset * width + xoffset) * bytes_per_pixel;

        if location + height * width * bytes_per_pixel > self.iface.frame.len() {
            return;
        }

        let x0 = rect.x.max(0) as usize;
        let y0 = rect.y.max(0) as usize;
        let x1 = (rect.right().max(0) as usize).min(width);
        let y1 = (rect.bottom().max(0) as usize).min(height);

        // Write pixmap to framebuffer with 180° rotation and BGRA format
        for y in y0..y1 {
            for x in x0..x1 {
                let idx = y * width + x;
                let pixel = self.pixmap.pixels()[idx];

                // Apply 180° rotation when writing to framebuffer
                let fb_x = width - x - 1;
                let fb_y = height - y - 1;
                let fb_idx = location + (fb_y * width + fb_x) * bytes_per_pixel;

                // Write as BGRA (use premultiplied values directly)
                self.iface.frame[fb_idx] = pixel.blue();
                self.iface.frame[fb_idx + 1] = pixel.green();
                self.iface.frame[fb_idx + 2] = pixel.red();
                self.iface.frame[fb_idx + 3] = pixel.alpha();
            }
        }
    }
}

/// One row of the stamp: where it goes within a page, and its slice of `Stamp::bytes`
struct StampRow {
    offset: usize,
    start: usize,
    len: usize,
}

/// Rows of a rect in fb byte order, to be repeated across every page the app may flip to
struct Stamp {
    /// Every row back to back, so a pass reads the source linearly. Shared with the holder so the
    /// content can be swapped without restarting the stamping thread.
    bytes: HeldPixels,
    rows: Box<[StampRow]>,
    pages: usize,
    page_stride: usize,
}

impl Stamp {
    fn blit(&self, frame: &mut [u8]) {
        let Ok(bytes) = self.bytes.lock() else {
            return;
        };
        for page in 0..self.pages {
            let base = page * self.page_stride;
            for row in &self.rows {
                let at = base + row.offset;
                if let Some(dst) = frame.get_mut(at..at + row.len) {
                    dst.copy_from_slice(&bytes[row.start..row.start + row.len]);
                }
            }
        }
    }
}

impl Display for FramebufferDisplay {
    fn width(&self) -> u32 {
        self.pixmap.width()
    }

    fn height(&self) -> u32 {
        self.pixmap.height()
    }

    fn pixmap(&self) -> PixmapRef<'_> {
        self.pixmap.as_ref()
    }

    fn pixmap_mut(&mut self) -> PixmapMut<'_> {
        self.pixmap.as_mut()
    }

    fn sync(&mut self) -> Result<()> {
        self.iface.var_screen_info = Framebuffer::get_var_screeninfo(&self.iface.device)
            .map_err(|e| anyhow!("failed to get var_screen_info: {}", e))?;

        let xoffset = self.iface.var_screen_info.xoffset as usize;
        let yoffset = self.iface.var_screen_info.yoffset as usize;
        let width = self.width() as usize;
        let height = self.height() as usize;
        let bytes_per_pixel = (self.iface.var_screen_info.bits_per_pixel / 8) as usize;
        let location = (yoffset * width + xoffset) * bytes_per_pixel;

        let frame = self.bounding_box();
        self.read_frame_rect(frame);

        if yoffset != 0 {
            let frame_size = width * height * bytes_per_pixel;
            self.iface
                .frame
                .copy_within(location..location + frame_size, 0);
            self.iface.var_screen_info.yoffset = 0;
            Framebuffer::put_var_screeninfo(&self.iface.device, &self.iface.var_screen_info)
                .map_err(|e| anyhow!("failed to set var_screen_info: {}", e))?;
        }

        Ok(())
    }

    fn map_pixels<F>(&mut self, mut f: F) -> Result<()>
    where
        F: FnMut(Color) -> Color,
    {
        for pixel in self.pixmap.pixels_mut() {
            let color: Color = (*pixel).into();
            *pixel = f(color).into();
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        self.write_rect(self.bounding_box());
        Ok(())
    }

    fn flush_rect(&mut self, area: Rect) -> Result<()> {
        // Re-read offsets: the foreground app may have moved yoffset since creation
        self.iface.var_screen_info = Framebuffer::get_var_screeninfo(&self.iface.device)
            .map_err(|e| anyhow!("failed to get var_screen_info: {}", e))?;
        self.write_rect(area);
        Ok(())
    }

    fn read_rect(&mut self, area: Rect) -> Result<()> {
        // Re-read offsets: the foreground app may have moved yoffset since creation
        self.iface.var_screen_info = Framebuffer::get_var_screeninfo(&self.iface.device)
            .map_err(|e| anyhow!("failed to get var_screen_info: {}", e))?;
        self.read_frame_rect(area);
        Ok(())
    }

    fn hold_rect(&mut self, area: Rect, corner_radius: u32) -> Result<Option<RectHold>> {
        let Some(stamp) = self.stamp(area, corner_radius) else {
            return Ok(None);
        };

        // Open the device before committing to a hold. Doing it inside the thread meant a failure
        // there left the caller believing a stamper was running, so it never fell back to
        // flushing the plate itself and nothing was ever drawn.
        let mut iface = match Framebuffer::new("/dev/fb0") {
            Ok(iface) => iface,
            Err(e) => {
                warn!("cannot open /dev/fb0 to stamp: {}", e);
                return Ok(None);
            }
        };

        // Waiting on vblank is the only trigger that is right whether or not the app pans, so
        // prefer it -- but only when the driver really blocks on it. Paired with each vblank is the
        // deadline that actually matters: the moment the panel reads the plate's own rows. The
        // panel is mounted upside down, so a rect at logical `area` is scanned starting at
        // `height - area.bottom()` rows in.
        let height = self.height().max(1);
        let vblank = measure_vblank(iface.device.as_raw_fd()).map(|frame| {
            let first_row = (height as i32 - area.bottom()).clamp(0, height as i32) as u32;
            (frame, frame * first_row / height)
        });
        // At info level deliberately: two attempts at this flicker have turned on which of these
        // branches the device actually takes, and RUST_LOG is info in the boot script.
        match vblank {
            Some((frame, pre_scanout)) => info!(
                "stamping on vblank: {frame:?} frame, sweeping the {pre_scanout:?} before the plate is scanned"
            ),
            None => info!("stamping by page polling: no vblank wait that actually blocks"),
        }
        let pre_scanout = vblank.map(|(_, pre_scanout)| pre_scanout);

        // The hold keeps a handle on the same buffer the thread reads, so later content changes
        // go through `refresh_rect_hold` instead of stopping this thread and starting another
        let pixels = Arc::clone(&stamp.bytes);
        Ok(Some(RectHold::spawn(pixels, move |stop| {
            // Paint every page up front so the plate is present whichever one the app shows next
            stamp.blit(&mut iface.frame);

            let fd = iface.device.as_raw_fd();
            let mut last_yoffset = iface.var_screen_info.yoffset;
            let mut logged_poll_error = false;
            let mut last_blit = Instant::now();

            while !stop.load(Ordering::Relaxed) {
                if let Some(pre_scanout) = pre_scanout {
                    // Blocks in the kernel, so this costs nothing until the panel is ready
                    if wait_for_vsync(fd).is_err() {
                        std::thread::sleep(STAMP_POLL);
                    }
                    // Then sweep from vblank up to the moment the plate is read. Blitting once at
                    // vblank -- what this did before -- is too early: the app draws its frame
                    // *after* that and wipes the plate, so the panel scans a gap. Blitting once at
                    // the deadline instead is too late whenever the app's draw runs long. Sweeping
                    // costs a few small memcpys and makes the last write before the plate is
                    // scanned ours, whatever the app's frame timing.
                    let start = Instant::now();
                    loop {
                        stamp.blit(&mut iface.frame);
                        let left = pre_scanout.saturating_sub(start.elapsed());
                        if left.is_zero() {
                            break;
                        }
                        std::thread::sleep(left.min(STAMP_SWEEP));
                    }
                    continue;
                }

                std::thread::sleep(STAMP_POLL);
                match Framebuffer::get_var_screeninfo(&iface.device) {
                    Ok(var) if var.yoffset != last_yoffset => {
                        last_yoffset = var.yoffset;
                        stamp.blit(&mut iface.frame);
                        last_blit = Instant::now();
                    }
                    Ok(_) => {
                        // An app that composites into the visible page never moves yoffset, so a
                        // flip is never seen. Repaint on a floor regardless or the plate is lost.
                        if last_blit.elapsed() >= STAMP_FLOOR {
                            stamp.blit(&mut iface.frame);
                            last_blit = Instant::now();
                        }
                    }
                    Err(e) => {
                        // Silently ignoring this looks exactly like the plate never being drawn
                        if !logged_poll_error {
                            logged_poll_error = true;
                            warn!(
                                "cannot read fb page offset, stamping on a timer only: {}",
                                e
                            );
                        }
                        if last_blit.elapsed() >= STAMP_FLOOR {
                            stamp.blit(&mut iface.frame);
                            last_blit = Instant::now();
                        }
                    }
                }
            }
        })))
    }

    fn refresh_rect_hold(
        &mut self,
        hold: &RectHold,
        area: Rect,
        corner_radius: u32,
    ) -> Result<bool> {
        let Some(stamp) = self.stamp(area, corner_radius) else {
            return Ok(false);
        };
        let Ok(bytes) = stamp.bytes.lock() else {
            return Ok(false);
        };
        // Publish first: flushing before this would leave the stamper repainting the previous
        // value over the one just written
        if !hold.update_pixels(&bytes) {
            return Ok(false);
        }
        // Dropping the guard matters: blit locks the same mutex
        drop(bytes);
        // Show it now rather than waiting for the app's next flip
        stamp.blit(&mut self.iface.frame);
        Ok(true)
    }

    fn save(&mut self) -> Result<()> {
        self.saved.push(self.pixmap.clone());
        Ok(())
    }

    fn load(&mut self, mut rect: Rect) -> Result<()> {
        let Some(saved) = self.saved.last() else {
            bail!("No saved image");
        };

        let size = self.size();
        if rect.x < 0
            || rect.y < 0
            || rect.x as u32 + rect.w > size.w
            || rect.y as u32 + rect.h > size.h
        {
            warn!(
                "Area exceeds display bounds: x: {}, y: {}, w: {}, h: {}",
                rect.x, rect.y, rect.w, rect.h,
            );
            rect.x = rect.x.max(0);
            rect.y = rect.y.max(0);
            rect.w = rect.w.min(size.w - rect.x as u32);
            rect.h = rect.h.min(size.h - rect.y as u32);
        }

        // Copy saved region to current pixmap
        let width = self.width() as usize;
        for dy in 0..rect.h {
            for dx in 0..rect.w {
                let x = (rect.x + dx as i32) as usize;
                let y = (rect.y + dy as i32) as usize;
                let idx = y * width + x;
                self.pixmap.pixels_mut()[idx] = saved.pixels()[idx];
            }
        }

        Ok(())
    }

    fn pop(&mut self) -> bool {
        self.saved.pop();
        !self.saved.is_empty()
    }
}
