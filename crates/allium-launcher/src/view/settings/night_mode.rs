use std::collections::VecDeque;

use anyhow::Result;
use async_trait::async_trait;
use common::command::{Command, Value};
use common::display::settings::{DEFAULT_NIGHT_MODE_STRENGTH, DisplaySettings};
use common::geom::{Alignment, Point, Rect};
use common::locale::Locale;
use common::platform::{DefaultPlatform, Key, KeyEvent, Platform};
use common::resources::Resources;
use common::stylesheet::Stylesheet;
use common::view::{ButtonHint, ButtonHints, Percentage, SettingsList, Toggle, View};
use tokio::sync::mpsc::Sender;

use crate::view::settings::{ChildState, SettingsChild};

const ROW_ENABLED: usize = 0;
const ROW_WARMTH: usize = 1;
const ROW_DIMNESS: usize = 2;
const ROW_LUMINANCE: usize = 3;
const ROW_HUE: usize = 4;
const ROW_SATURATION: usize = 5;
const ROW_CONTRAST: usize = 6;
const ROW_RED: usize = 7;
const ROW_GREEN: usize = 8;
const ROW_BLUE: usize = 9;

/// The platform raises anything below this, so don't offer values the panel will never show
const MIN_CONTRAST: i32 = 10;

fn percentage(value: i32, min: i32) -> Box<dyn View> {
    Box::new(Percentage::new(
        Point::zero(),
        value.max(min),
        min,
        100,
        Alignment::Right,
    ))
}

pub struct NightMode {
    rect: Rect,
    settings: DisplaySettings,
    list: SettingsList,
    button_hints: ButtonHints<String>,
}

impl NightMode {
    pub fn new(rect: Rect, res: Resources, state: Option<ChildState>) -> Self {
        let Rect { x, y, w, .. } = rect;

        let settings = DisplaySettings::load().unwrap_or_default();

        let locale = res.get::<Locale>();
        let styles = res.get::<Stylesheet>();

        let mut button_hints = ButtonHints::new(
            res.clone(),
            vec![ButtonHint::new(
                res.clone(),
                Point::zero(),
                Key::X,
                locale.t("button-restore-defaults"),
                Alignment::Left,
            )],
            vec![
                ButtonHint::new(
                    res.clone(),
                    Point::zero(),
                    Key::A,
                    locale.t("button-edit"),
                    Alignment::Right,
                ),
                ButtonHint::new(
                    res.clone(),
                    Point::zero(),
                    Key::B,
                    locale.t("button-back"),
                    Alignment::Right,
                ),
            ],
        );

        // Lays the hints out, which is what makes their rect meaningful
        let button_hints_rect = button_hints.bounding_box(&styles);
        let list_height = (button_hints_rect.y - y) as u32;

        let mut list = SettingsList::new(
            res.clone(),
            Rect::new(
                x + styles.ui.margin_x,
                y,
                w - styles.ui.margin_x as u32 * 2,
                list_height,
            ),
            vec![
                locale.t("settings-night-mode-enabled"),
                locale.t("settings-night-mode-warmth"),
                locale.t("settings-night-mode-dimness"),
                locale.t("settings-display-luminance"),
                locale.t("settings-display-hue"),
                locale.t("settings-display-saturation"),
                locale.t("settings-display-contrast"),
                locale.t("settings-display-red"),
                locale.t("settings-display-green"),
                locale.t("settings-display-blue"),
            ],
            vec![
                Box::new(Toggle::new(
                    Point::zero(),
                    settings.night_mode,
                    Alignment::Right,
                )),
                Box::new(Percentage::new(
                    Point::zero(),
                    i32::from(settings.night_mode_warmth),
                    0,
                    100,
                    Alignment::Right,
                )),
                Box::new(Percentage::new(
                    Point::zero(),
                    i32::from(settings.night_mode_dimness),
                    0,
                    100,
                    Alignment::Right,
                )),
                percentage(i32::from(settings.luminance), 0),
                percentage(i32::from(settings.hue), 0),
                percentage(i32::from(settings.saturation), 0),
                percentage(i32::from(settings.contrast), MIN_CONTRAST),
                percentage(i32::from(settings.r), 0),
                percentage(i32::from(settings.g), 0),
                percentage(i32::from(settings.b), 0),
            ],
            styles.ui.ui_font.size + styles.ui.padding_y as u32,
        );
        if let Some(state) = state {
            list.select(state.selected);
        }

        drop(locale);
        drop(styles);

        Self {
            rect,
            settings,
            list,
            button_hints,
        }
    }

    /// Applies one edited row on top of whatever is currently on disk.
    ///
    /// Re-reading matters because the daemon owns the same file and flips `night_mode` from the
    /// Menu+Select hotkey while this screen is open. Setting only the edited field -- rather than
    /// listing every field to preserve -- means a field added later cannot be silently dropped.
    fn apply_row(&mut self, row: usize, val: &Value) -> Option<DisplaySettings> {
        let mut settings = DisplaySettings::load().unwrap_or_else(|_| self.settings.clone());
        let percent = || val.clone().as_int().unwrap_or(0).clamp(0, 100) as u8;
        match row {
            ROW_ENABLED => settings.night_mode = val.clone().as_bool().unwrap_or(false),
            ROW_WARMTH => settings.night_mode_warmth = percent(),
            ROW_DIMNESS => settings.night_mode_dimness = percent(),
            ROW_LUMINANCE => settings.luminance = percent(),
            ROW_HUE => settings.hue = percent(),
            ROW_SATURATION => settings.saturation = percent(),
            ROW_CONTRAST => settings.contrast = percent().max(MIN_CONTRAST as u8),
            ROW_RED => settings.r = percent(),
            ROW_GREEN => settings.g = percent(),
            ROW_BLUE => settings.b = percent(),
            _ => return None,
        }
        self.settings = settings.clone();
        Some(settings)
    }
}

#[async_trait(?Send)]
impl View for NightMode {
    fn draw(
        &mut self,
        display: &mut <DefaultPlatform as Platform>::Display,
        styles: &Stylesheet,
    ) -> Result<bool> {
        let mut drawn = false;

        drawn |= self.list.should_draw() && self.list.draw(display, styles)?;
        drawn |= self.button_hints.should_draw() && self.button_hints.draw(display, styles)?;

        Ok(drawn)
    }

    fn should_draw(&self) -> bool {
        self.list.should_draw() || self.button_hints.should_draw()
    }

    fn set_should_draw(&mut self) {
        self.list.set_should_draw();
        self.button_hints.set_should_draw();
    }

    async fn handle_key_event(
        &mut self,
        event: KeyEvent,
        commands: Sender<Command>,
        bubble: &mut VecDeque<Command>,
    ) -> Result<bool> {
        if self
            .list
            .handle_key_event(event, commands.clone(), bubble)
            .await?
        {
            while let Some(command) = bubble.pop_front() {
                match command {
                    // Mid-edit: drive the panel so the change is visible as it is made, but
                    // don't touch the SD card
                    Command::ValuePreview(i, val) => {
                        if let Some(settings) = self.apply_row(i, &val) {
                            commands
                                .send(Command::ApplyDisplaySettings(Box::new(settings)))
                                .await?;
                        }
                    }
                    Command::ValueChanged(i, val) => {
                        if let Some(settings) = self.apply_row(i, &val) {
                            commands
                                .send(Command::SaveDisplaySettings(Box::new(settings)))
                                .await?;
                        }
                    }
                    _ => {}
                }
            }
            return Ok(true);
        }

        match event {
            KeyEvent::Pressed(Key::X) => {
                // Restore the whole screen, colour rows included, so one press undoes a session
                // of tuning rather than only the two strength sliders
                let defaults = DisplaySettings::default();
                let mut settings = DisplaySettings::load().unwrap_or_else(|_| defaults.clone());
                settings.night_mode_warmth = DEFAULT_NIGHT_MODE_STRENGTH;
                settings.night_mode_dimness = DEFAULT_NIGHT_MODE_STRENGTH;
                settings.luminance = defaults.luminance;
                settings.hue = defaults.hue;
                settings.saturation = defaults.saturation;
                settings.contrast = defaults.contrast;
                settings.r = defaults.r;
                settings.g = defaults.g;
                settings.b = defaults.b;
                self.settings = settings.clone();

                for (row, value, min) in [
                    (ROW_WARMTH, i32::from(DEFAULT_NIGHT_MODE_STRENGTH), 0),
                    (ROW_DIMNESS, i32::from(DEFAULT_NIGHT_MODE_STRENGTH), 0),
                    (ROW_LUMINANCE, i32::from(defaults.luminance), 0),
                    (ROW_HUE, i32::from(defaults.hue), 0),
                    (ROW_SATURATION, i32::from(defaults.saturation), 0),
                    (ROW_CONTRAST, i32::from(defaults.contrast), MIN_CONTRAST),
                    (ROW_RED, i32::from(defaults.r), 0),
                    (ROW_GREEN, i32::from(defaults.g), 0),
                    (ROW_BLUE, i32::from(defaults.b), 0),
                ] {
                    self.list.set_right(row, percentage(value, min));
                }

                commands
                    .send(Command::SaveDisplaySettings(Box::new(settings)))
                    .await?;
                Ok(true)
            }
            KeyEvent::Pressed(Key::B) => {
                bubble.push_back(Command::CloseView);
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    fn children(&self) -> Vec<&dyn View> {
        vec![&self.list, &self.button_hints]
    }

    fn children_mut(&mut self) -> Vec<&mut dyn View> {
        vec![&mut self.list, &mut self.button_hints]
    }

    fn bounding_box(&mut self, _styles: &Stylesheet) -> Rect {
        self.rect
    }

    fn set_position(&mut self, _point: Point) {
        unimplemented!()
    }
}

impl SettingsChild for NightMode {
    fn save(&self) -> ChildState {
        ChildState {
            selected: self.list.selected(),
        }
    }
}
