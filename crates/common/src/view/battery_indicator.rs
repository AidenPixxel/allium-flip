use std::collections::VecDeque;
use std::time::{Duration, Instant};

use anyhow::Result;
use async_trait::async_trait;
use log::error;
use tokio::sync::mpsc::Sender;

use crate::battery::Battery;
use crate::constants::BATTERY_UPDATE_INTERVAL;
use crate::geom::{Point, Rect};
use crate::platform::{DefaultPlatform, KeyEvent, Platform};
use crate::resources::Resources;
use crate::stylesheet::Stylesheet;
use crate::view::{BatteryIcon, Command, Label, View};

#[derive(Debug, Clone)]
struct BatteryState {
    charging: bool,
    percentage: i32,
}

#[derive(Debug, Clone)]
pub struct BatteryIndicator<B>
where
    B: Battery + 'static,
{
    res: Resources,
    point: Point,
    last_state: BatteryState,
    last_updated: Instant,
    label: Option<Label<String>>,
    battery: B,
    icon: BatteryIcon,
}

impl<B> BatteryIndicator<B>
where
    B: Battery + 'static,
{
    pub fn new(res: Resources, point: Point, mut battery: B, show_percentage: bool) -> Self {
        battery.update().unwrap();

        let label = if show_percentage {
            let styles = res.get::<Stylesheet>();
            let mut label = Label::new(
                point,
                format_battery_percentage(battery.charging(), battery.percentage()),
                crate::geom::Alignment::Right,
                None,
            );
            label.font_size(styles.status_bar.font_size);
            label.color(crate::stylesheet::StylesheetColor::StatusBar);
            label.stroke_color(crate::stylesheet::StylesheetColor::StatusBarStroke);
            Some(label)
        } else {
            None
        };

        let icon = BatteryIcon::new(point);

        Self {
            res,
            point,
            last_state: BatteryState {
                charging: battery.charging(),
                percentage: battery.percentage(),
            },
            last_updated: Instant::now(),
            label,
            battery,
            icon,
        }
    }

    fn layout(&mut self) {
        let styles = self.res.get::<Stylesheet>();
        let label_w = if let Some(ref mut label) = self.label {
            label.bounding_box(&styles).w as i32
        } else {
            0
        };
        let label_w = if label_w > 0 {
            label_w + styles.ui.margin_x
        } else {
            0
        };

        self.icon
            .set_state(self.battery.charging(), self.battery.percentage());
        self.icon
            .set_position(Point::new(self.point.x - label_w, self.point.y));
    }
}

#[async_trait(?Send)]
impl<B> View for BatteryIndicator<B>
where
    B: Battery,
{
    fn update(&mut self, _dt: Duration) {
        if self.last_updated.elapsed() < BATTERY_UPDATE_INTERVAL {
            return;
        }
        self.last_updated = Instant::now();
        if let Err(e) = self.battery.update() {
            error!("Failed to update battery: {}", e);
        }

        if self.battery.charging() == self.last_state.charging
            && self.battery.percentage() == self.last_state.percentage
        {
            return;
        }

        if let Some(ref mut label) = self.label {
            label.set_text(format_battery_percentage(
                self.battery.charging(),
                self.battery.percentage(),
            ));
        }

        self.layout();
        self.set_should_draw();
    }

    fn draw(
        &mut self,
        display: &mut <DefaultPlatform as Platform>::Display,
        styles: &Stylesheet,
    ) -> Result<bool> {
        let mut drawn = false;

        drawn |= self.icon.should_draw() && self.icon.draw(display, styles)?;
        if let Some(ref mut label) = self.label {
            drawn |= label.should_draw() && label.draw(display, styles)?;
        }

        Ok(drawn)
    }

    fn should_draw(&self) -> bool {
        self.icon.should_draw() || self.label.as_ref().is_some_and(|l| l.should_draw())
    }

    fn set_should_draw(&mut self) {
        self.icon.set_should_draw();
        if let Some(ref mut label) = self.label {
            label.set_should_draw()
        }
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
        if let Some(ref label) = self.label {
            vec![label, &self.icon]
        } else {
            vec![&self.icon]
        }
    }

    fn children_mut(&mut self) -> Vec<&mut dyn View> {
        if let Some(ref mut label) = self.label {
            vec![label, &mut self.icon]
        } else {
            vec![&mut self.icon]
        }
    }

    fn bounding_box(&mut self, styles: &Stylesheet) -> Rect {
        let label_w = if let Some(ref mut label) = self.label {
            label.bounding_box(styles).w as i32
        } else {
            0
        };
        let label_w = if label_w > 0 {
            label_w + styles.ui.margin_x
        } else {
            0
        };

        let icon_bbox = self.icon.bounding_box(styles);
        // Extend bounding box to include label width
        Rect::new(
            icon_bbox.x,
            icon_bbox.y,
            icon_bbox.w + label_w as u32,
            icon_bbox.h,
        )
    }

    fn set_position(&mut self, point: Point) {
        self.point = point;
        if let Some(ref mut label) = self.label {
            label.set_position(point);
        }
        self.layout();
    }
}

fn format_battery_percentage(charging: bool, percentage: i32) -> String {
    if charging {
        String::new()
    } else {
        format!("{}%", percentage)
    }
}
