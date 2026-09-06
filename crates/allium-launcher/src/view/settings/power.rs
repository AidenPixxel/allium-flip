use std::collections::VecDeque;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use common::command::{Command, Value};

use common::display::Display as DisplayTrait;
use common::geom::{Alignment, Point, Rect};
use common::locale::Locale;
use common::platform::{DefaultPlatform, Key, KeyEvent, Platform};
use common::power::{ChargingBootAction, PowerButtonAction, PowerSettings, VolumeOnStartup};
use common::resources::Resources;
use common::stylesheet::Stylesheet;
use common::view::{ButtonHint, ButtonHints, Label, Number, Select, SettingsList, Toggle, View};

use tokio::sync::mpsc::Sender;

use crate::view::settings::{ChildState, SettingsChild};

pub struct Power {
    res: Resources,
    rect: Rect,
    power_settings: PowerSettings,
    /// The options offered by the charging boot action row, in display order. The list is
    /// device-dependent, so the Select index has to be mapped back through it.
    charging_boot_actions: Vec<ChargingBootAction>,
    list: SettingsList,
    /// Explains the highlighted row. Several of these settings are not self-describing -- "Charge
    /// Silently" and "Nothing" in particular -- and getting one wrong is only discovered later,
    /// when the device does something unexpected while you are not looking at it.
    description: Label<String>,
    description_rect: Rect,
    button_hints: ButtonHints<String>,
}

/// Locale key describing the option currently chosen for `row`, or `None` for rows whose label
/// already says everything.
fn description_key(row: usize, settings: &PowerSettings) -> Option<&'static str> {
    match row {
        ROW_AUTO_SLEEP_CHARGING => Some(if settings.auto_sleep_when_charging {
            "settings-power-desc-auto-sleep-when-charging-on"
        } else {
            "settings-power-desc-auto-sleep-when-charging-off"
        }),
        ROW_AUTO_SLEEP_MINUTES => Some(if settings.auto_sleep_duration_minutes == 0 {
            "settings-power-desc-auto-sleep-duration-disabled"
        } else {
            "settings-power-desc-auto-sleep-duration"
        }),
        ROW_CHARGING_BOOT => Some(match settings.charging_boot_action {
            ChargingBootAction::ChargeScreen => "settings-power-desc-charging-boot-charge-screen",
            ChargingBootAction::ChargeSilently => {
                "settings-power-desc-charging-boot-charge-silently"
            }
            ChargingBootAction::PowerOff => "settings-power-desc-charging-boot-power-off",
        }),
        ROW_VOLUME_ON_STARTUP => Some(match settings.volume_on_startup {
            VolumeOnStartup::Restore => "settings-power-desc-volume-on-startup-restore",
            VolumeOnStartup::Muted => "settings-power-desc-volume-on-startup-muted",
        }),
        ROW_POWER_BUTTON | ROW_LID_CLOSE => Some(match power_action(row, settings) {
            PowerButtonAction::Suspend => "settings-power-desc-action-suspend",
            PowerButtonAction::Shutdown => "settings-power-desc-action-shutdown",
            PowerButtonAction::Nothing => "settings-power-desc-action-nothing",
        }),
        _ => None,
    }
}

fn power_action(row: usize, settings: &PowerSettings) -> PowerButtonAction {
    if row == ROW_LID_CLOSE {
        settings.lid_close_action
    } else {
        settings.power_button_action
    }
}

const ROW_AUTO_SLEEP_CHARGING: usize = 0;
const ROW_AUTO_SLEEP_MINUTES: usize = 1;
const ROW_CHARGING_BOOT: usize = 2;
const ROW_VOLUME_ON_STARTUP: usize = 3;
const ROW_POWER_BUTTON: usize = 4;
const ROW_LID_CLOSE: usize = 5;

/// Powering off is hidden where `shutdown` can only reboot, which would make plugging in a
/// charger loop the device through boot forever.
fn charging_boot_actions() -> Vec<ChargingBootAction> {
    let mut actions = vec![
        ChargingBootAction::ChargeScreen,
        ChargingBootAction::ChargeSilently,
    ];
    if DefaultPlatform::can_power_off() {
        actions.push(ChargingBootAction::PowerOff);
    }
    actions
}

fn charging_boot_action_key(action: ChargingBootAction) -> &'static str {
    match action {
        ChargingBootAction::ChargeScreen => "settings-power-charging-boot-action-charge-screen",
        ChargingBootAction::ChargeSilently => "settings-power-charging-boot-action-charge-silently",
        ChargingBootAction::PowerOff => "settings-power-charging-boot-action-power-off",
    }
}

impl Power {
    pub fn new(rect: Rect, res: Resources, state: Option<ChildState>) -> Self {
        let Rect { x, y, w, .. } = rect;

        let locale = res.get::<Locale>();
        let styles = res.get::<Stylesheet>();
        let power_settings = PowerSettings::load().unwrap_or_default();

        let auto_sleep_duration_disabled_label =
            locale.t("settings-power-auto-sleep-duration-disabled");

        let charging_boot_actions = charging_boot_actions();

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
        let row_pitch =
            styles.ui.ui_font.size + styles.ui.padding_y as u32 + styles.ui.list_margin as u32;
        let rows = if DefaultPlatform::has_lid() { 6 } else { 5 };
        // Take the description's strip out of the list, but never so much that SettingsList's
        // visible_count drops a row and starts scrolling
        let available = (button_hints_rect.y - y) as u32;
        let description_height = styles.ui.ui_font.size + styles.ui.padding_y as u32;
        let list_height = available
            .saturating_sub(description_height + styles.ui.margin_y as u32)
            .max((rows * row_pitch).min(available));
        let description_rect = Rect::new(
            x + styles.ui.margin_x,
            y + list_height as i32,
            w - styles.ui.margin_x as u32 * 2,
            description_height,
        );

        let mut buttons: Vec<(String, Box<dyn View>)> = vec![
            (
                locale.t("settings-power-auto-sleep-when-charging"),
                Box::new(Toggle::new(
                    Point::zero(),
                    power_settings.auto_sleep_when_charging,
                    Alignment::Right,
                )),
            ),
            (
                locale.t("settings-power-auto-sleep-duration-minutes"),
                Box::new(Number::new(
                    Point::zero(),
                    power_settings.auto_sleep_duration_minutes,
                    0,
                    60,
                    5,
                    move |x: &i32| {
                        if *x == 0 {
                            auto_sleep_duration_disabled_label.clone()
                        } else {
                            x.to_string()
                        }
                    },
                    Alignment::Right,
                )),
            ),
            (
                locale.t("settings-power-charging-boot-action"),
                Box::new(Select::new(
                    Point::zero(),
                    charging_boot_actions
                        .iter()
                        .position(|a| *a == power_settings.charging_boot_action)
                        .unwrap_or_default(),
                    charging_boot_actions
                        .iter()
                        .map(|a| locale.t(charging_boot_action_key(*a)))
                        .collect(),
                    Alignment::Right,
                )),
            ),
            (
                locale.t("settings-power-volume-on-startup"),
                Box::new(Select::new(
                    Point::zero(),
                    power_settings.volume_on_startup as usize,
                    vec![
                        locale.t("settings-power-volume-on-startup-restore"),
                        locale.t("settings-power-volume-on-startup-muted"),
                    ],
                    Alignment::Right,
                )),
            ),
            (
                locale.t("settings-power-power-button-action"),
                Box::new(Select::new(
                    Point::zero(),
                    power_settings.power_button_action as usize,
                    vec![
                        locale.t("settings-power-power-button-action-suspend"),
                        locale.t("settings-power-power-button-action-shutdown"),
                        locale.t("settings-power-power-button-action-nothing"),
                    ],
                    Alignment::Right,
                )),
            ),
        ];
        if DefaultPlatform::has_lid() {
            buttons.push((
                locale.t("settings-power-lid-close-action"),
                Box::new(Select::new(
                    Point::zero(),
                    power_settings.lid_close_action as usize,
                    vec![
                        locale.t("settings-power-power-button-action-suspend"),
                        locale.t("settings-power-power-button-action-shutdown"),
                        locale.t("settings-power-power-button-action-nothing"),
                    ],
                    Alignment::Right,
                )),
            ));
        }
        let (left, right) = buttons.into_iter().unzip();

        let mut list = SettingsList::new(
            res.clone(),
            Rect::new(
                x + styles.ui.margin_x,
                y,
                w - styles.ui.margin_x as u32 * 2,
                list_height,
            ),
            left,
            right,
            styles.ui.ui_font.size + styles.ui.padding_y as u32,
        );
        if let Some(state) = state {
            list.select(state.selected);
        }

        let description_text = description_key(list.selected(), &power_settings)
            .map(|key| locale.t(key))
            .unwrap_or_default();
        let description = Label::new(
            Point::new(description_rect.x, description_rect.y),
            description_text,
            Alignment::Left,
            None,
        );

        drop(locale);
        drop(styles);

        Self {
            res,
            rect,
            power_settings,
            charging_boot_actions,
            list,
            description,
            description_rect,
            button_hints,
        }
    }
}

impl Power {
    fn apply_value(&mut self, row: usize, val: Value) {
        match row {
            ROW_AUTO_SLEEP_CHARGING => {
                self.power_settings.auto_sleep_when_charging = val.as_bool().unwrap_or(true)
            }
            ROW_AUTO_SLEEP_MINUTES => {
                self.power_settings.auto_sleep_duration_minutes = val.as_int().unwrap_or(5)
            }
            ROW_CHARGING_BOOT => {
                // The option list is device-dependent, so index through it rather than
                // treating the Select index as the enum discriminant.
                self.power_settings.charging_boot_action = self
                    .charging_boot_actions
                    .get(val.as_int().unwrap_or(0) as usize)
                    .copied()
                    .unwrap_or_default();
            }
            ROW_VOLUME_ON_STARTUP => {
                self.power_settings.volume_on_startup =
                    VolumeOnStartup::from_repr(val.as_int().unwrap_or(0) as usize)
                        .unwrap_or_default()
            }
            ROW_POWER_BUTTON => {
                self.power_settings.power_button_action =
                    PowerButtonAction::from_repr(val.as_int().unwrap_or(0) as usize)
                        .unwrap_or_default()
            }
            ROW_LID_CLOSE => {
                self.power_settings.lid_close_action =
                    PowerButtonAction::from_repr(val.as_int().unwrap_or(0) as usize)
                        .unwrap_or_default()
            }
            _ => {}
        }
    }

    fn refresh_description(&mut self) {
        let text = description_key(self.list.selected(), &self.power_settings)
            .map(|key| self.res.get::<Locale>().t(key))
            .unwrap_or_default();
        self.description.set_text(text);
    }
}

#[async_trait(?Send)]
impl View for Power {
    fn draw(
        &mut self,
        display: &mut <DefaultPlatform as Platform>::Display,
        styles: &Stylesheet,
    ) -> Result<bool> {
        let mut drawn = false;

        drawn |= self.list.should_draw() && self.list.draw(display, styles)?;

        if self.description.should_draw() {
            // The description sits between the list's rect and the hints', so neither restores it
            display.load(self.description_rect)?;
            drawn |= self.description.draw(display, styles)?;
        }

        if self.button_hints.should_draw() {
            let bbox = self.button_hints.bounding_box(styles);
            display.load(Rect::new(
                self.rect.x,
                bbox.y - styles.ui.margin_x,
                self.rect.w,
                bbox.h,
            ))?;
            drawn |= self.button_hints.draw(display, styles)?;
        }

        Ok(drawn)
    }

    fn should_draw(&self) -> bool {
        self.list.should_draw() || self.description.should_draw() || self.button_hints.should_draw()
    }

    fn set_should_draw(&mut self) {
        self.list.set_should_draw();
        self.description.set_should_draw();
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
                    // Cycling the options: follow along so the description tracks the highlighted
                    // option, but don't persist until it is committed
                    Command::ValuePreview(i, val) => {
                        self.apply_value(i, val);
                    }
                    Command::ValueChanged(i, val) => {
                        self.apply_value(i, val);
                        self.power_settings.save()?;
                        toast_needs_restart_for_effect(&self.res, &commands).await?;
                    }
                    _ => {}
                }
            }
            // The list consumed the event, so either the highlighted row moved or the value on
            // it changed. Both alter what should be described, and neither is signalled directly.
            self.refresh_description();
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
        vec![&self.list, &self.description, &self.button_hints]
    }

    fn children_mut(&mut self) -> Vec<&mut dyn View> {
        vec![
            &mut self.list,
            &mut self.description,
            &mut self.button_hints,
        ]
    }

    fn bounding_box(&mut self, _styles: &Stylesheet) -> Rect {
        self.rect
    }

    fn set_position(&mut self, _point: Point) {
        unimplemented!()
    }
}

async fn toast_needs_restart_for_effect(res: &Resources, commands: &Sender<Command>) -> Result<()> {
    let message = res.get::<Locale>().t("settings-needs-restart-for-effect");
    Ok(commands
        .send(Command::Toast(message, Some(Duration::from_secs(5))))
        .await?)
}

impl SettingsChild for Power {
    fn save(&self) -> ChildState {
        ChildState {
            selected: self.list.selected(),
        }
    }
}
