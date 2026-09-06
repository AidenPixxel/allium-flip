use std::collections::VecDeque;

use anyhow::Result;
use async_trait::async_trait;
use common::command::Command;
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

    /// Re-reads the stored settings before applying an edit. The daemon owns the same file and
    /// flips `night_mode` from the Menu+Select hotkey, so a cached struct would write a stale
    /// value back over it.
    fn merged(&mut self) -> DisplaySettings {
        if let Ok(current) = DisplaySettings::load() {
            self.settings = DisplaySettings {
                night_mode: self.settings.night_mode,
                night_mode_warmth: self.settings.night_mode_warmth,
                night_mode_dimness: self.settings.night_mode_dimness,
                ..current
            };
        }
        self.settings.clone()
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
                        let value = val.as_int().unwrap_or(0).clamp(0, 100) as u8;
                        match i {
                            ROW_WARMTH => self.settings.night_mode_warmth = value,
                            ROW_DIMNESS => self.settings.night_mode_dimness = value,
                            _ => continue,
                        }
                        let settings = self.merged();
                        commands
                            .send(Command::ApplyDisplaySettings(Box::new(settings)))
                            .await?;
                    }
                    Command::ValueChanged(i, val) => {
                        match i {
                            ROW_ENABLED => {
                                self.settings.night_mode = val.as_bool().unwrap_or(false)
                            }
                            ROW_WARMTH => {
                                self.settings.night_mode_warmth =
                                    val.as_int().unwrap_or(0).clamp(0, 100) as u8
                            }
                            ROW_DIMNESS => {
                                self.settings.night_mode_dimness =
                                    val.as_int().unwrap_or(0).clamp(0, 100) as u8
                            }
                            _ => continue,
                        }
                        let settings = self.merged();
                        commands
                            .send(Command::SaveDisplaySettings(Box::new(settings)))
                            .await?;
                    }
                    _ => {}
                }
            }
            return Ok(true);
        }

        match event {
            KeyEvent::Pressed(Key::X) => {
                self.settings.night_mode_warmth = DEFAULT_NIGHT_MODE_STRENGTH;
                self.settings.night_mode_dimness = DEFAULT_NIGHT_MODE_STRENGTH;
                self.list.set_right(
                    ROW_WARMTH,
                    Box::new(Percentage::new(
                        Point::zero(),
                        i32::from(DEFAULT_NIGHT_MODE_STRENGTH),
                        0,
                        100,
                        Alignment::Right,
                    )),
                );
                self.list.set_right(
                    ROW_DIMNESS,
                    Box::new(Percentage::new(
                        Point::zero(),
                        i32::from(DEFAULT_NIGHT_MODE_STRENGTH),
                        0,
                        100,
                        Alignment::Right,
                    )),
                );
                let settings = self.merged();
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
