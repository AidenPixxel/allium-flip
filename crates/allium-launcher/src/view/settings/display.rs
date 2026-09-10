use std::collections::VecDeque;

use anyhow::Result;
use async_trait::async_trait;
use common::command::{Command, Value};

use common::constants::MIN_BRIGHTNESS;
use common::display::settings::{DisplaySettings, MAX_PROFILE_NAME_LEN, MIN_CONTRAST};
use common::geom::{Alignment, Point, Rect, Size};
use common::locale::Locale;
use common::platform::{DefaultPlatform, Key, KeyEvent, Platform};
use common::resources::Resources;
use common::stylesheet::Stylesheet;
use common::view::{
    ButtonHint, ButtonHints, Label, Percentage, Select, SettingsList, TextBox, View,
};

use tokio::sync::mpsc::Sender;

use crate::view::settings::{ChildState, SettingsChild};

const ROW_PROFILE: usize = 0;
const ROW_NAME: usize = 1;
const ROW_RESOLUTION: usize = 2;
const ROW_LUMINANCE: usize = 3;
const ROW_HUE: usize = 4;
const ROW_SATURATION: usize = 5;
const ROW_CONTRAST: usize = 6;
const ROW_RED: usize = 7;
const ROW_GREEN: usize = 8;
const ROW_BLUE: usize = 9;
const ROW_WARMTH: usize = 10;
const ROW_BRIGHTNESS: usize = 11;

fn percentage(value: u8, min: u8) -> Box<dyn View> {
    Box::new(Percentage::new(
        Point::zero(),
        i32::from(value.max(min)),
        i32::from(min),
        100,
        Alignment::Right,
    ))
}

pub struct Display {
    rect: Rect,
    res: Resources,
    settings: DisplaySettings,
    list: SettingsList,
    button_hints: ButtonHints<String>,
    edit_button: Option<ButtonHint<String>>,
}

impl Display {
    pub fn new(rect: Rect, res: Resources, state: Option<ChildState>) -> Self {
        let Rect { x, y, w, .. } = rect;

        let settings = DisplaySettings::load().unwrap_or_default();

        let locale = res.get::<Locale>();
        let styles = res.get::<Stylesheet>();

        // Row 0 is the profile selector, which is editable, so the Edit hint starts on screen and
        // is only taken away while the read-only resolution row is highlighted
        let mut button_hints = ButtonHints::new(
            res.clone(),
            vec![],
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

        let button_hints_rect = button_hints.bounding_box(&styles);
        let list_height = (button_hints_rect.y - y) as u32;

        let edit_button = None;

        let profile = settings.active().clone();
        let mut list = SettingsList::new(
            res.clone(),
            Rect::new(
                x + styles.ui.margin_x,
                y,
                w - styles.ui.margin_x as u32 * 2,
                list_height,
            ),
            vec![
                locale.t("settings-display-profile"),
                locale.t("settings-display-profile-name"),
                locale.t("settings-display-screen-resolution"),
                locale.t("settings-display-luminance"),
                locale.t("settings-display-hue"),
                locale.t("settings-display-saturation"),
                locale.t("settings-display-contrast"),
                locale.t("settings-display-red"),
                locale.t("settings-display-green"),
                locale.t("settings-display-blue"),
                locale.t("settings-display-warmth"),
                locale.t("settings-display-brightness"),
            ],
            vec![
                Box::new(Select::new(
                    Point::zero(),
                    settings.active,
                    settings.names(),
                    Alignment::Right,
                )),
                Box::new(TextBox::new(
                    Point::zero(),
                    res.clone(),
                    settings.name_of(settings.active),
                    Alignment::Right,
                    false,
                )),
                Box::new(Label::new(
                    Point::zero(),
                    {
                        let size = res.get::<Size>();
                        format!("{}x{}", size.w, size.h)
                    },
                    Alignment::Right,
                    None,
                )),
                percentage(profile.luminance, 0),
                percentage(profile.hue, 0),
                percentage(profile.saturation, 0),
                percentage(profile.contrast, MIN_CONTRAST),
                percentage(profile.r, 0),
                percentage(profile.g, 0),
                percentage(profile.b, 0),
                percentage(profile.warmth, 0),
                percentage(profile.brightness, MIN_BRIGHTNESS),
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
            res,
            settings,
            list,
            button_hints,
            edit_button,
        }
    }

    /// Applies one edited row on top of whatever is on disk, and returns the document to write.
    ///
    /// Re-reading matters because the daemon rotates the active profile from the Menu+Select
    /// hotkey while this screen is open. Setting only the edited field -- rather than listing
    /// every field to preserve -- means a field added later cannot be silently dropped.
    fn apply_row(&mut self, row: usize, val: &Value) -> Option<DisplaySettings> {
        let mut settings = DisplaySettings::load().unwrap_or_else(|_| self.settings.clone());
        let percent = || val.clone().as_int().unwrap_or(0).clamp(0, 100) as u8;

        match row {
            ROW_PROFILE => {
                let index = val.clone().as_int().unwrap_or(0).max(0) as usize;
                settings.active = index.min(settings.profiles.len() - 1);
            }
            ROW_NAME => {
                let name: String = val
                    .clone()
                    .as_string()
                    .unwrap_or_default()
                    .chars()
                    .take(MAX_PROFILE_NAME_LEN)
                    .collect();
                settings.active_mut().name = name;
            }
            ROW_LUMINANCE => settings.active_mut().luminance = percent(),
            ROW_HUE => settings.active_mut().hue = percent(),
            ROW_SATURATION => settings.active_mut().saturation = percent(),
            ROW_CONTRAST => settings.active_mut().contrast = percent().max(MIN_CONTRAST),
            ROW_RED => settings.active_mut().r = percent(),
            ROW_GREEN => settings.active_mut().g = percent(),
            ROW_BLUE => settings.active_mut().b = percent(),
            ROW_WARMTH => settings.active_mut().warmth = percent(),
            ROW_BRIGHTNESS => settings.active_mut().brightness = percent().max(MIN_BRIGHTNESS),
            _ => return None,
        }

        self.settings = settings.clone();
        Some(settings)
    }

    /// Repoints every value row at the active profile, after switching or renaming one.
    fn reload_rows(&mut self) {
        let profile = self.settings.active().clone();
        for (row, value, min) in [
            (ROW_LUMINANCE, profile.luminance, 0),
            (ROW_HUE, profile.hue, 0),
            (ROW_SATURATION, profile.saturation, 0),
            (ROW_CONTRAST, profile.contrast, MIN_CONTRAST),
            (ROW_RED, profile.r, 0),
            (ROW_GREEN, profile.g, 0),
            (ROW_BLUE, profile.b, 0),
            (ROW_WARMTH, profile.warmth, 0),
            (ROW_BRIGHTNESS, profile.brightness, MIN_BRIGHTNESS),
        ] {
            self.list.set_right(row, percentage(value, min));
        }

        // Select bakes its labels in at construction, so a rename needs a fresh one
        self.list.set_right(
            ROW_PROFILE,
            Box::new(Select::new(
                Point::zero(),
                self.settings.active,
                self.settings.names(),
                Alignment::Right,
            )),
        );
        self.list.set_right(
            ROW_NAME,
            Box::new(TextBox::new(
                Point::zero(),
                self.res.clone(),
                self.settings.name_of(self.settings.active),
                Alignment::Right,
                false,
            )),
        );
    }
}

#[async_trait(?Send)]
impl View for Display {
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
            // The resolution row is a read-only label, so there is nothing to edit on it
            if self.list.selected() == ROW_RESOLUTION && self.button_hints.right().len() == 2 {
                self.edit_button = Some(self.button_hints.right_mut().remove(0));
            } else if let Some(button) = self.edit_button.take()
                && self.button_hints.right().len() == 1
            {
                self.button_hints.right_mut().insert(0, button);
            }

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
                            // Switching or renaming a profile changes what every other row shows
                            if i == ROW_PROFILE || i == ROW_NAME {
                                self.reload_rows();
                            }
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

impl SettingsChild for Display {
    fn save(&self) -> ChildState {
        ChildState {
            selected: self.list.selected(),
        }
    }
}
