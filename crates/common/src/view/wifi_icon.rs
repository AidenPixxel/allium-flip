use std::collections::VecDeque;
use std::path::PathBuf;

use anyhow::Result;
use async_trait::async_trait;
use image::RgbaImage;
use log::debug;
use tiny_skia::{BlendMode, Paint, PathBuilder, Transform};
use tokio::sync::mpsc::Sender;

use crate::constants::ALLIUM_THEMES_DIR;
use crate::display::Display;
use crate::geom::{Point, Rect};
use crate::platform::{DefaultPlatform, KeyEvent, Platform};
use crate::stylesheet::{Stylesheet, Theme};
use crate::view::{Command, View};

/// Layout calculations for vector WiFi icon.
/// All positions are absolute screen coordinates.
struct VectorWifiLayout {
    /// Center point of the WiFi icon
    center: Point,
    /// Radii for each arc (3 arcs for signal strength)
    radii: [f32; 3],
    /// Arc angles
    start_angle: f32,
    end_angle: f32,
    /// Stroke width
    stroke_width: f32,
    /// Dot radius for the base
    dot_radius: f32,
}

impl VectorWifiLayout {
    fn calculate(point: Point, styles: &Stylesheet) -> Self {
        let font_size = styles.status_bar_font_size();

        // Match battery icon dimensions
        let content_h = font_size * 0.6;
        let stroke_width = (font_size / 15.0).max(1.0);

        // Vertical centering (same as battery icon)
        let y_offset = (font_size - content_h) / 2.0;
        let content_top = point.y as f32 + y_offset;

        // Dot radius (dot diameter will be 2 * stroke_width)
        let dot_radius = stroke_width;

        // Arc radii (3 levels of signal) - scaled to fit within content_h
        // Need to account for dot at bottom
        let max_radius = (content_h - dot_radius) * 0.9;

        // Position center so top arc is at content_top and dot fits at bottom
        // Top of arc: center_y - max_radius = content_top
        let center_y = content_top + content_h - dot_radius;

        // Horizontal positioning
        let icon_width = content_h;
        let center = Point::new(point.x - (icon_width * 0.5) as i32, center_y as i32);

        // Equal spacing between dot and arcs, and between arcs
        // With dot_radius = stroke_width, for equal gaps:
        // r1 = (max_radius + stroke_width) / 3
        // r2 = 2*max_radius/3 + stroke_width/6
        // r3 = max_radius
        let radii = [
            (max_radius + stroke_width) / 3.0,
            2.0 * max_radius / 3.0 + stroke_width / 6.0,
            max_radius,
        ];

        // Arc angles (bottom half circle)
        let start_angle = std::f32::consts::PI * 0.75;
        let end_angle = std::f32::consts::PI * 0.25;

        Self {
            center,
            radii,
            start_angle,
            end_angle,
            stroke_width,
            dot_radius,
        }
    }

    fn total_size(styles: &Stylesheet) -> Rect {
        let font_size = styles.status_bar_font_size();
        let base_size = font_size * 0.6;

        let w = base_size as i32;
        let h = font_size as i32;

        Rect::new(0, 0, w as u32, h as u32)
    }
}

#[derive(Debug, Clone)]
enum WifiIconVariant {
    Image {
        connected: RgbaImage,
        disconnected: RgbaImage,
    },
    Vector,
}

#[derive(Debug, Clone)]
pub struct WifiIcon {
    variant: WifiIconVariant,
    point: Point,
    connected: bool,
    dirty: bool,
}

impl WifiIcon {
    pub fn new(point: Point) -> Self {
        Self {
            variant: Self::load_variant(),
            point,
            connected: false,
            dirty: true,
        }
    }

    fn load_variant() -> WifiIconVariant {
        let theme = Theme::load();
        let theme_dir = ALLIUM_THEMES_DIR.join(&theme.0);

        let resolve_icon_path = |icon_name: &str| -> PathBuf {
            let theme_icon = theme_dir.join("assets").join(icon_name);
            if theme_icon.exists() {
                return theme_icon;
            }
            ALLIUM_THEMES_DIR
                .join("Allium")
                .join("assets")
                .join(icon_name)
        };

        let connected_path = resolve_icon_path("wifi-connected.png");
        let connected = match image::open(connected_path) {
            Ok(img) => img.to_rgba8(),
            Err(e) => {
                debug!(
                    "Failed to load WiFi connected icon: {}. Falling back to vector rendering.",
                    e
                );
                return WifiIconVariant::Vector;
            }
        };

        let disconnected_path = resolve_icon_path("wifi-disconnected.png");
        let disconnected = match image::open(disconnected_path) {
            Ok(img) => img.to_rgba8(),
            Err(e) => {
                debug!(
                    "Failed to load WiFi disconnected icon: {}. Falling back to vector rendering.",
                    e
                );
                return WifiIconVariant::Vector;
            }
        };

        WifiIconVariant::Image {
            connected,
            disconnected,
        }
    }

    pub fn set_connected(&mut self, connected: bool) {
        if self.connected != connected {
            self.connected = connected;
            self.dirty = true;
        }
    }

    fn icon_size(&self, styles: &Stylesheet) -> Rect {
        match &self.variant {
            WifiIconVariant::Image {
                connected: connected_img,
                disconnected: disconnected_img,
            } => {
                let img = if self.connected {
                    connected_img
                } else {
                    disconnected_img
                };
                Rect::new(0, 0, img.width(), img.height())
            }
            WifiIconVariant::Vector => VectorWifiLayout::total_size(styles),
        }
    }

    fn draw_icon(
        &self,
        display: &mut <DefaultPlatform as Platform>::Display,
        styles: &Stylesheet,
    ) -> Result<()> {
        match &self.variant {
            WifiIconVariant::Image {
                connected: connected_img,
                disconnected: disconnected_img,
            } => {
                let image_to_draw = if self.connected {
                    connected_img
                } else {
                    disconnected_img
                };

                let icon_width = image_to_draw.width() as i32;
                let draw_point = Point::new(self.point.x - icon_width, self.point.y);

                crate::display::image::draw_image(
                    &mut display.pixmap_mut(),
                    image_to_draw,
                    draw_point,
                );
            }
            WifiIconVariant::Vector => {
                let layout = VectorWifiLayout::calculate(self.point, styles);
                let color = if self.connected {
                    styles.status_bar.text_color
                } else {
                    styles.ui.disabled_color
                };
                let mut pixmap = display.pixmap_mut();

                let paint = Paint {
                    shader: tiny_skia::Shader::SolidColor(color.into()),
                    blend_mode: BlendMode::SourceOver,
                    anti_alias: true,
                    ..Default::default()
                };

                // Draw three arcs for signal strength
                for &radius in layout.radii.iter() {
                    let mut pb = PathBuilder::new();

                    // Create arc path
                    let num_segments = 20;
                    let angle_step = (layout.end_angle - layout.start_angle) / num_segments as f32;

                    for j in 0..=num_segments {
                        let angle = layout.start_angle + angle_step * j as f32;
                        let x = layout.center.x as f32 + radius * angle.cos();
                        let y = layout.center.y as f32 - radius * angle.sin();

                        if j == 0 {
                            pb.move_to(x, y);
                        } else {
                            pb.line_to(x, y);
                        }
                    }

                    if let Some(path) = pb.finish() {
                        let stroke = tiny_skia::Stroke {
                            width: layout.stroke_width,
                            ..Default::default()
                        };
                        pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
                    }
                }

                // Draw center dot
                let mut pb = PathBuilder::new();
                pb.push_circle(
                    layout.center.x as f32,
                    layout.center.y as f32,
                    layout.dot_radius,
                );
                if let Some(path) = pb.finish() {
                    pixmap.fill_path(
                        &path,
                        &paint,
                        tiny_skia::FillRule::Winding,
                        Transform::identity(),
                        None,
                    );
                }

                if !self.connected {
                    // Draw slash across the WiFi icon when disconnected
                    let max_radius = layout.radii[2];
                    let horizontal_offset = max_radius * 0.6;

                    // Slash from bottom-left to top-right, aligned with icon bounds
                    let start_x = layout.center.x as f32 - horizontal_offset + layout.stroke_width;
                    let start_y = layout.center.y as f32 + layout.dot_radius;
                    let end_x = layout.center.x as f32 + horizontal_offset - layout.stroke_width;
                    let end_y = layout.center.y as f32 - max_radius - layout.dot_radius;

                    let mut pb = PathBuilder::new();
                    pb.move_to(start_x, start_y);
                    pb.line_to(end_x, end_y);
                    if let Some(path) = pb.finish() {
                        let stroke = tiny_skia::Stroke {
                            width: layout.stroke_width,
                            ..Default::default()
                        };
                        pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
                    }
                }
            }
        }

        Ok(())
    }
}

#[async_trait(?Send)]
impl View for WifiIcon {
    fn draw(
        &mut self,
        display: &mut <DefaultPlatform as Platform>::Display,
        styles: &Stylesheet,
    ) -> Result<bool> {
        if !self.dirty {
            return Ok(false);
        }
        self.draw_icon(display, styles)?;
        self.dirty = false;
        Ok(true)
    }

    fn should_draw(&self) -> bool {
        self.dirty
    }

    fn set_should_draw(&mut self) {
        self.dirty = true;
    }

    async fn handle_key_event(
        &mut self,
        _event: KeyEvent,
        _commands: Sender<Command>,
        _bubble: &mut VecDeque<Command>,
    ) -> Result<bool> {
        Ok(false)
    }

    fn children(&self) -> Vec<&dyn View> {
        vec![]
    }

    fn children_mut(&mut self) -> Vec<&mut dyn View> {
        vec![]
    }

    fn bounding_box(&mut self, styles: &Stylesheet) -> Rect {
        let icon_size = self.icon_size(styles);
        let left = self.point.x - icon_size.w as i32;
        let top = self.point.y;

        Rect::new(left, top, icon_size.w, icon_size.h)
    }

    fn set_position(&mut self, point: Point) {
        self.point = point;
        self.dirty = true;
    }
}
