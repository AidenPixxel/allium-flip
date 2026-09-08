use std::collections::{BTreeMap, VecDeque};
use std::fs;

use anyhow::Result;
use async_trait::async_trait;
use common::command::{Command, Value};
use common::constants::RELAUNCH_MARKER;
use common::database::Database;
use common::display::Display;
use common::game_info::GameInfo;
use common::geom::{Alignment, Point, Rect};
use common::locale::Locale;
use common::performance::{self, PerformanceMode};
use common::platform::{DefaultPlatform, Key, KeyEvent, Platform};
use common::power::PowerSettings;
use common::resources::Resources;
use common::retroarch::RetroArchCommand;
use common::retroarch_config::OverrideScope;
use common::retroarch_options::{Overrides, PERFORMANCE_KEY, Setting, Target};
use common::stylesheet::Stylesheet;
use common::view::{ButtonHint, ButtonHints, Label, Select, SettingsList, View};
use log::warn;
use tokio::sync::mpsc::Sender;

/// Row 0 is always the scope picker; the settings start below it.
const ROW_SCOPE: usize = 0;

/// A screen of RetroArch override rows, driven by whichever table it is handed.
///
/// Both Controls and Options are this view with a different `settings` table, which is why adding
/// a setting is a table entry rather than a new screen.
pub struct OverrideSettings {
    rect: Rect,
    res: Resources,
    settings: &'static [Setting],
    overrides: Overrides,
    /// The scopes offered, in display order. A ROM at the top of the Roms directory has no folder,
    /// so Console is not always among them and the Select index has to be mapped back through this.
    scopes: Vec<OverrideScope>,
    scope: OverrideScope,
    list: SettingsList,
    description: Label<String>,
    description_rect: Rect,
    button_hints: ButtonHints<String>,
}

fn scope_key(scope: OverrideScope) -> &'static str {
    match scope {
        OverrideScope::Game => "override-scope-game",
        OverrideScope::Console => "override-scope-console",
        OverrideScope::Core => "override-scope-core",
    }
}

impl OverrideSettings {
    pub fn new(
        rect: Rect,
        res: Resources,
        settings: &'static [Setting],
        overrides: Overrides,
    ) -> Self {
        let Rect { x, y, w, .. } = rect;

        let locale = res.get::<Locale>();
        let styles = res.get::<Stylesheet>();

        let mut button_hints = ButtonHints::new(
            res.clone(),
            // On the left, away from Edit and Back, because it acts on the whole screen rather
            // than the highlighted row -- and because its presence is what tells you a change is
            // waiting for a relaunch at all.
            vec![ButtonHint::new(
                res.clone(),
                Point::zero(),
                Key::Y,
                locale.t("override-apply"),
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

        let scopes: Vec<OverrideScope> = [
            OverrideScope::Game,
            OverrideScope::Console,
            OverrideScope::Core,
        ]
        .into_iter()
        .filter(|scope| overrides.supports(*scope))
        .collect();
        let scope = scopes.first().copied().unwrap_or_default();

        let button_hints_rect = button_hints.bounding_box(&styles);
        let available = (button_hints_rect.y - y) as u32;
        let description_height = styles.ui.ui_font.size + styles.ui.padding_y as u32;
        let list_height = available.saturating_sub(description_height + styles.ui.margin_y as u32);
        let description_rect = Rect::new(
            x + styles.ui.margin_x,
            y + list_height as i32,
            w - styles.ui.margin_x as u32 * 2,
            description_height,
        );

        let mut labels = vec![locale.t("override-scope")];
        let mut rights: Vec<Box<dyn View>> = vec![Box::new(Select::new(
            Point::zero(),
            0,
            scopes.iter().map(|s| locale.t(scope_key(*s))).collect(),
            Alignment::Right,
        ))];

        for setting in settings {
            labels.push(locale.t(setting.label));
            rights.push(Box::new(Select::new(
                Point::zero(),
                0,
                setting
                    .choices
                    .iter()
                    .map(|choice| locale.t(choice.label))
                    .collect(),
                Alignment::Right,
            )));
        }

        let list = SettingsList::new(
            res.clone(),
            Rect::new(
                x + styles.ui.margin_x,
                y,
                w - styles.ui.margin_x as u32 * 2,
                list_height,
            ),
            labels,
            rights,
            styles.ui.ui_font.size + styles.ui.padding_y as u32,
        );

        // No width, and no scrolling: the in-game menu's event loop has no frame timer and never
        // calls `update`, so a marquee here would sit frozen. These strings have to be short
        // enough to fit instead.
        let description = Label::new(
            Point::new(description_rect.x, description_rect.y),
            String::new(),
            Alignment::Left,
            None,
        );

        drop(locale);
        drop(styles);

        let mut this = Self {
            rect,
            res,
            settings,
            overrides,
            scopes,
            scope,
            list,
            description,
            description_rect,
            button_hints,
        };
        // One code path for the initial values and for a scope change
        this.reload_rows();
        this.refresh_description();
        this
    }

    /// The setting a row edits, or `None` for the scope row.
    fn setting(&self, row: usize) -> Option<&'static Setting> {
        row.checked_sub(1).and_then(|i| self.settings.get(i))
    }

    /// What a setting currently reads as, from whichever store backs it.
    fn state_of(&self, setting: &Setting) -> BTreeMap<String, String> {
        match setting.target {
            Target::Allium => {
                // Synthesised into the same shape a config file would give, so `current` needs no
                // special case for a setting Allium stores itself.
                let mut map = BTreeMap::new();
                let mode = self
                    .res
                    .get::<Database>()
                    .get_performance_mode(self.overrides.rom())
                    .unwrap_or_default()
                    .filter(|mode| *mode != PerformanceMode::System);
                if let Some(mode) = mode {
                    map.insert(PERFORMANCE_KEY.to_owned(), mode.name().to_owned());
                }
                map
            }
            target => self.overrides.read(self.scope, target),
        }
    }

    /// Persists a performance mode the way the in-game row used to.
    ///
    /// Three writes, because three things need it: the database remembers it for next launch, the
    /// governor takes it now, and the state file carries it so a resume does not re-apply whatever
    /// was resolved when the game started.
    fn write_performance(&self, choice: usize, setting: &Setting) {
        let mode = setting
            .changes(choice)
            .into_iter()
            .find(|(key, _)| *key == PERFORMANCE_KEY)
            .and_then(|(_, value)| value)
            .and_then(|value| PerformanceMode::from_name(&value));

        if let Err(err) = self
            .res
            .get::<Database>()
            .set_performance_mode(self.overrides.rom(), mode)
        {
            warn!("could not save the performance mode: {err}");
        }

        let effective =
            mode.unwrap_or_else(|| PowerSettings::load().unwrap_or_default().performance_mode);
        performance::apply(effective);

        if let Err(err) = GameInfo::store_performance_mode(effective) {
            warn!("could not record the performance mode for resume: {err}");
        }
    }

    /// Quits the game and has the daemon start it again, so the settings just written take effect.
    ///
    /// The only way to apply them: RetroArch reads its override files when it loads content and
    /// its command interface has no setter -- all forty-odd commands are actions. Quitting is also
    /// what makes RetroArch write its auto-save, so with Auto Save on this lands the player back
    /// roughly where they were.
    async fn apply_now(&self, commands: &Sender<Command>) -> Result<()> {
        // Written before the quit, because the daemon reads it the moment the child exits
        if let Err(err) = fs::write(RELAUNCH_MARKER, "") {
            warn!("could not ask for a relaunch, quitting to the launcher instead: {err}");
        }

        commands
            .send(Command::RetroArchCommand(RetroArchCommand::Quit))
            .await?;
        commands.send(Command::Exit).await?;
        Ok(())
    }

    fn refresh_description(&mut self) {
        let locale = self.res.get::<Locale>();
        let text = match self.setting(self.list.selected()) {
            Some(setting) => locale.t(setting.description),
            None => locale.t("override-desc-scope"),
        };
        drop(locale);
        self.description.set_text(text);
    }

    /// Rebuilds every value row against the newly chosen scope.
    ///
    /// Each row shows what is set at *this* scope only, so switching from Game to Core swaps the
    /// whole screen's values rather than leaving the previous tier's showing.
    fn reload_rows(&mut self) {
        let locale = self.res.get::<Locale>();
        let rows: Vec<(usize, usize, Vec<String>)> = self
            .settings
            .iter()
            .enumerate()
            .map(|(i, setting)| {
                let map = self.state_of(setting);
                (
                    i + 1,
                    setting.current(&map),
                    setting
                        .choices
                        .iter()
                        .map(|choice| locale.t(choice.label))
                        .collect(),
                )
            })
            .collect();
        drop(locale);

        for (row, current, choices) in rows {
            self.list.set_right(
                row,
                Box::new(Select::new(
                    Point::zero(),
                    current,
                    choices,
                    Alignment::Right,
                )),
            );
        }
    }

    /// Writes a committed choice straight through to disk.
    ///
    /// Eagerly, not on close: the menu throws its whole view tree away at the start of every
    /// session, so anything held in memory is simply lost.
    fn commit(&self, row: usize, choice: usize) {
        let Some(setting) = self.setting(row) else {
            return;
        };

        if setting.target == Target::Allium {
            self.write_performance(choice, setting);
            return;
        }

        if let Err(err) = self.overrides.apply(self.scope, setting, choice) {
            warn!("could not write {}: {err}", setting.label);
        }
    }
}

#[async_trait(?Send)]
impl View for OverrideSettings {
    fn draw(
        &mut self,
        display: &mut <DefaultPlatform as Platform>::Display,
        styles: &Stylesheet,
    ) -> Result<bool> {
        let mut drawn = false;

        drawn |= self.list.should_draw() && self.list.draw(display, styles)?;

        if self.description.should_draw() {
            // Sits between the list and the hints, so neither of them restores it
            display.load(self.description_rect)?;
            drawn |= self.description.draw(display, styles)?;
        }

        if self.button_hints.should_draw() {
            display.load(self.button_hints.bounding_box(styles))?;
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
                if let Command::ValueChanged(row, Value::Int(value)) = command {
                    let value = value.max(0) as usize;
                    if row == ROW_SCOPE {
                        let current = self.scope;
                        self.scope = self.scopes.get(value).copied().unwrap_or(current);
                        self.reload_rows();
                    } else {
                        self.commit(row, value);
                    }
                }
            }

            // The list consumed the event, so either the highlight moved or a value changed --
            // both alter what should be described, and neither is signalled directly.
            self.refresh_description();
            return Ok(true);
        }

        match event {
            KeyEvent::Pressed(Key::Y) => {
                self.apply_now(&commands).await?;
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
