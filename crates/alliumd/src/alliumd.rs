use std::collections::VecDeque;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Instant;

use allium_menu::{AlliumMenu, RetroArchInfo};
use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use common::battery::Battery;
use common::constants::{
    ALLIUM_GAME_INFO, ALLIUM_LAUNCHER, ALLIUM_SD_ROOT, ALLIUM_VERSION, ALLIUMD_STATE,
    BATTERY_SHUTDOWN_THRESHOLD, BATTERY_UPDATE_INTERVAL, BATTERY_WARNING_THRESHOLD,
    CHARGE_POWER_OFF_GRACE, MAX_BRIGHTNESS, MAX_VOLUME,
};
use common::display::settings::DisplaySettings;
use common::locale::{Locale, LocaleSettings};
use common::performance;
use common::power::{ChargingBootAction, PowerButtonAction, PowerSettings, VolumeOnStartup};
use common::retroarch::RetroArchCommand;
use common::stylesheet::Stylesheet;
use common::wifi::WiFiSettings;
use enum_map::EnumMap;
use log::{debug, error, info, trace, warn};
use serde::{Deserialize, Serialize};
use tokio::process::{Child, Command};

use common::database::Database;
use common::game_info::GameInfo;
use common::platform::{DefaultPlatform, Key, KeyEvent, Platform};

use crate::osd::{Osd, OsdContent, OsdKind};

#[cfg(unix)]
use {
    nix::sys::signal::Signal, nix::sys::signal::kill, nix::unistd::Pid,
    tokio::signal::unix::SignalKind,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlliumDState {
    #[serde(default = "Utc::now")]
    time: DateTime<Utc>,
    volume: i32,
    brightness: u8,
}

/// Handle to the persistent menu thread
struct MenuHandle {
    tx: mpsc::Sender<Option<RetroArchInfo>>,
    done_rx: tokio::sync::mpsc::UnboundedReceiver<()>,
    _handle: JoinHandle<Result<()>>,
}

impl MenuHandle {
    fn new(styles: Stylesheet) -> Self {
        let (tx, rx) = mpsc::channel::<Option<RetroArchInfo>>();
        let (done_tx, done_rx) = tokio::sync::mpsc::unbounded_channel();
        let rt = tokio::runtime::Handle::current();

        let handle = std::thread::spawn(move || -> Result<()> {
            rt.block_on(async {
                let platform = common::platform::DefaultPlatform::new()?;
                let mut app = AlliumMenu::new(platform, styles).await?;

                while let Ok(info) = rx.recv() {
                    if let Err(e) = app.prepare(info).await {
                        log::error!("menu prepare failed: {:?}", e);
                    } else if let Err(e) = app.run_event_loop().await {
                        log::error!("menu run failed: {:?}", e);
                    }
                    if let Err(e) = app.save() {
                        log::error!("menu save failed: {:?}", e);
                    }
                    let _ = done_tx.send(());
                }
                Ok(())
            })
        });

        MenuHandle {
            tx,
            done_rx,
            _handle: handle,
        }
    }
}

pub struct AlliumD<P: Platform> {
    platform: P,
    main: Child,
    menu: MenuHandle,
    menu_open: bool,
    keys: EnumMap<Key, bool>,
    is_menu_pressed_alone: bool,
    is_terminating: bool,
    state: AlliumDState,
    locale: Locale,
    power_settings: PowerSettings,
    osd: Osd<P>,
    /// When the child last exited, within `CRASH_LOOP_WINDOW`; see `throttle_crash_loop`
    child_exits: VecDeque<Instant>,
}

impl AlliumDState {
    pub fn new() -> Self {
        Self {
            time: Utc::now(),
            volume: 20,
            brightness: 50,
        }
    }

    pub fn load() -> Result<AlliumDState> {
        if ALLIUMD_STATE.exists() {
            debug!("found state, loading from file");
            if let Ok(json) = fs::read_to_string(ALLIUMD_STATE.as_path())
                && let Ok(this) = serde_json::from_str::<AlliumDState>(&json)
            {
                if Utc::now() < this.time {
                    info!(
                        "RTC is not working, advancing time to {}",
                        this.time.format("%F %T")
                    );
                    let mut date = std::process::Command::new("date")
                        .arg("--utc")
                        .arg("--set")
                        .arg(this.time.format("%F %T").to_string())
                        .spawn()?;
                    date.wait()?;
                    let mut hwclock = std::process::Command::new("/sbin/hwclock")
                        .arg("--systohc")
                        .arg("--utc")
                        .arg(this.time.format("%F %T").to_string())
                        .spawn()?;
                    hwclock.wait()?;
                }
                return Ok(this);
            }
            warn!("failed to read state file, removing");
            fs::remove_file(ALLIUMD_STATE.as_path())?;
        }
        Ok(Self::new())
    }

    fn save(&self) -> Result<()> {
        let json = serde_json::to_string(self).unwrap();
        File::create(ALLIUMD_STATE.as_path())?.write_all(json.as_bytes())?;
        Ok(())
    }
}

/// Sleeps until `deadline`, or forever when there is none — the select! arm needs no precondition
async fn sleep_until_wake(deadline: Option<tokio::time::Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

/// Child exits within this window count toward the crash-loop guard...
const CRASH_LOOP_WINDOW: std::time::Duration = std::time::Duration::from_secs(10);
/// ...and this many of them means the child is not coming up: pause before the next attempt
const CRASH_LOOP_EXITS: usize = 3;
const CRASH_LOOP_BACKOFF: std::time::Duration = std::time::Duration::from_secs(5);
/// How long to wait before trying again when the spawn itself fails
const RESPAWN_RETRY: std::time::Duration = std::time::Duration::from_secs(5);

/// `spawn_main`, retried until it succeeds.
///
/// Failing to start the launcher is not something the daemon can recover from by exiting: the boot
/// script answers a dead daemon with a reboot, which lands straight back here. Waiting and trying
/// again is strictly better -- the card may still be settling after an update -- and keeps the
/// device answering the power button meanwhile.
async fn respawn_main() -> Child {
    loop {
        match spawn_main().await {
            Ok(child) => return child,
            Err(e) => {
                error!(
                    "could not start the launcher: {}; trying again in {:?}",
                    e, RESPAWN_RETRY
                );
                tokio::time::sleep(RESPAWN_RETRY).await;
            }
        }
    }
}

async fn spawn_main() -> Result<Child> {
    #[cfg(feature = "miyoo")]
    return Ok(match GameInfo::load()? {
        Some(mut game_info) => {
            debug!("found game info, resuming game");
            game_info.start_time = Utc::now();
            game_info.save()?;
            // The launcher does not run on this path, so nothing else would re-apply the
            // governor after a reboot. Infallible by design: this function's failures leave the
            // device in a reboot loop.
            performance::apply(game_info.performance_mode);
            game_info.command().into()
        }
        None => {
            debug!("no game info found, launching launcher");
            use common::constants::ALLIUM_LAUNCHER;
            performance::restore();
            Command::new(ALLIUM_LAUNCHER.as_path())
        }
    }
    .spawn()?);

    #[cfg(not(feature = "miyoo"))]
    return Ok(Command::new("/bin/sh")
        .arg("-c")
        .arg("make simulator-launcher")
        .spawn()?);
}

impl AlliumD<DefaultPlatform> {
    pub async fn new() -> Result<AlliumD<DefaultPlatform>> {
        // Before anything gets the chance to change it, so returning to the launcher can put the
        // governor back to whatever the kernel booted with
        performance::capture();

        #[cfg(feature = "miyoo")]
        common::platform::miyoo::try_fix_resolution().await?;

        let mut platform = DefaultPlatform::new()?;
        let mut state = AlliumDState::load()?;

        let mut keys = EnumMap::default();

        // Poll all key events for 100ms to sync initial key state
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_millis(100);
        loop {
            tokio::select! {
                key_event = platform.poll() => {
                    match key_event {
                        KeyEvent::Pressed(key) => keys[key] = true,
                        KeyEvent::Released(key) => keys[key] = false,
                        KeyEvent::Autorepeat(key) => keys[key] = true,
                    }
                }
                _ = tokio::time::sleep_until(deadline) => {
                    break;
                }
            }
        }

        if keys[Key::Menu] || keys[Key::B] {
            info!("menu or B key held at startup, bypass game resume");
            GameInfo::delete()?;
        }

        let power_settings = PowerSettings::load()?;

        state.volume = if keys[Key::VolDown] {
            info!("volume down key held at startup, muting");
            0
        } else {
            match power_settings.volume_on_startup {
                VolumeOnStartup::Muted => {
                    info!("volume on startup is muted");
                    0
                }
                VolumeOnStartup::Restore => state.volume,
            }
        };

        info!("setting volume: {}", state.volume);
        platform.set_volume(state.volume)?;

        info!("setting brightness: {}", state.brightness);
        platform.set_brightness(state.brightness)?;

        info!("loading display settings");
        // `effective` folds in the profile's warmth and dimness without baking them into the
        // stored values, so the active profile survives a reboot.
        platform.set_display_settings(&mut DisplaySettings::load()?.active().effective())?;

        let main = respawn_main().await;
        let locale = Locale::new(&LocaleSettings::load()?.lang);

        // One load per process: font data is Arc'd, so clones share it
        let styles = Stylesheet::load()?;

        // Spawn the persistent menu thread at startup
        let menu = MenuHandle::new(styles.clone());

        platform.daemon();

        Ok(AlliumD {
            platform,
            main,
            menu,
            menu_open: false,
            keys,
            is_menu_pressed_alone: false,
            is_terminating: false,
            state,
            locale,
            power_settings,
            osd: Osd::new(styles),
            child_exits: VecDeque::new(),
        })
    }

    pub async fn run_event_loop(&mut self) -> Result<()> {
        info!("hello from Allium {}", *ALLIUM_VERSION);

        if DefaultPlatform::has_wifi() {
            info!("wifi detected, loading wifi settings");
            WiFiSettings::load()?.init()?;
        }

        info!("starting event loop");
        #[cfg(unix)]
        {
            let mut sigint = tokio::signal::unix::signal(SignalKind::interrupt())?;
            let mut sigterm = tokio::signal::unix::signal(SignalKind::terminate())?;

            let mut battery_interval = Instant::now();

            // Charging at this point means the device was booted by the charger being plugged in
            // (or by the user pressing Power with the cable already attached).
            let mut battery = self.platform.battery()?;
            // Every error out of this function is a reboot, so a single failed reading -- axp_test
            // slow to answer, say -- must not be one. Not charging is the safe assumption.
            if let Err(e) = battery.update() {
                warn!("failed to read the battery at startup: {}", e);
            }
            if battery.charging() {
                match self.charging_boot_action() {
                    ChargingBootAction::ChargeScreen => self.handle_charging(true).await?,
                    ChargingBootAction::ChargeSilently => self.handle_charging(false).await?,
                    ChargingBootAction::PowerOff => self.handle_charging_power_off().await?,
                }
            }

            let mut battery_led_task = None;

            loop {
                if battery_interval.elapsed() >= BATTERY_UPDATE_INTERVAL {
                    battery_interval = Instant::now();
                    trace!("updating battery");
                    if let Err(e) = battery.update() {
                        error!("failed to update battery: {}", e);
                    }

                    if battery.percentage() <= BATTERY_WARNING_THRESHOLD && !battery.charging() {
                        if battery_led_task.is_none() {
                            warn!(
                                "battery is low ({}%), consider charging soon",
                                battery.percentage()
                            );

                            battery_led_task = Some(tokio::spawn(async {
                                loop {
                                    <DefaultPlatform as Platform>::Battery::update_led(true);
                                    tokio::time::sleep(std::time::Duration::from_millis(1750))
                                        .await;
                                    <DefaultPlatform as Platform>::Battery::update_led(false);
                                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                                }
                            }));
                        }
                    } else if let Some(task) = battery_led_task.take() {
                        info!("aborting battery LED blink task");
                        task.abort();
                        <DefaultPlatform as Platform>::Battery::update_led(false);
                    }

                    if battery.percentage() <= BATTERY_SHUTDOWN_THRESHOLD && !battery.charging() {
                        warn!("battery is low, shutting down");
                        self.handle_quit().await?;
                    }
                }

                let auto_sleep_duration = match self.power_settings.auto_sleep_duration_minutes {
                    0 => std::time::Duration::MAX, // disabled
                    t => std::time::Duration::new(t as u64 * 60, 0),
                };
                tokio::select! {
                    key_event = self.platform.poll() => {
                        // A failed myctl spawn on a volume press is not worth a reboot
                        if let Err(e) = self.handle_key_event(key_event).await {
                            error!("failed to handle key event: {}", e);
                        }
                    }
                    _ = sleep_until_wake(self.osd.next_wake()) => {
                        if let Err(e) = self.osd.tick() {
                            error!("failed to update OSD: {}", e);
                        }
                    }
                    // `Some` matters: once the menu thread is gone the channel closes and recv
                    // returns None forever, so a `_` pattern would spin the loop at full speed,
                    // flooding RetroArch with Unpause and starving the event loop
                    Some(()) = self.menu.done_rx.recv() => {
                        info!("menu finished, resuming game");
                        self.menu_open = false;
                        self.is_menu_pressed_alone = false;
                        RetroArchCommand::Unpause.send_or_log().await;
                    }
                    _ = tokio::time::sleep(auto_sleep_duration) => {
                        if !self.power_settings.auto_sleep_when_charging && battery.charging() {
                            info!("battery charging, don't auto sleep");
                        } else if Path::new("/tmp/stay_awake").exists() {
                            info!("/tmp/stay_awake exists, don't auto sleep");
                        } else {
                            info!("idle timeout, shutting down");
                            self.handle_quit().await?;
                        }
                    }
                    _ = self.main.wait() => {
                        if !self.is_terminating {
                            info!("main process terminated, recording play time");
                            if let Err(e) = self.update_play_time() {
                                error!("failed to record play time: {}", e);
                            }
                            if let Err(e) = GameInfo::delete() {
                                error!("failed to clear the game info: {}", e);
                            }
                            self.throttle_crash_loop().await;
                            self.main = respawn_main().await;
                        }
                    }
                    _ = sigint.recv() => self.handle_quit().await?,
                    _ = sigterm.recv() => self.handle_quit().await?,
                }
            }
        }

        #[cfg(not(unix))]
        loop {
            tokio::select! {
                key_event = self.platform.poll() => {
                    if let Err(e) = self.handle_key_event(key_event).await {
                        error!("failed to handle key event: {}", e);
                    }
                }
                _ = sleep_until_wake(self.osd.next_wake()) => {
                    if let Err(e) = self.osd.tick() {
                        error!("failed to update OSD: {}", e);
                    }
                }
            }
        }
    }

    async fn handle_key_event(&mut self, key_event: KeyEvent) -> Result<()> {
        debug!(
            "main: {:?}, ingame: {}, key_event: {:?}",
            self.main.id(),
            self.is_ingame(),
            key_event
        );

        // Handle menu key
        match key_event {
            KeyEvent::Pressed(Key::Menu) => {
                self.is_menu_pressed_alone = true;
            }
            KeyEvent::Pressed(_) => {
                self.is_menu_pressed_alone = false;
            }
            KeyEvent::Released(_) | KeyEvent::Autorepeat(_) => {}
        }

        // Update self.keys
        match key_event {
            KeyEvent::Pressed(key) => {
                self.keys[key] = true;
            }
            KeyEvent::Released(key) => {
                self.keys[key] = false;
            }
            KeyEvent::Autorepeat(_) => {}
        }

        if self.keys[Key::Menu] {
            // Global hotkeys
            match key_event {
                KeyEvent::Pressed(Key::Up | Key::VolUp)
                | KeyEvent::Autorepeat(Key::Up | Key::VolUp) => {
                    self.add_brightness(5)?;
                }
                KeyEvent::Pressed(Key::Down | Key::VolDown)
                | KeyEvent::Autorepeat(Key::Down | Key::VolDown) => {
                    self.add_brightness(-5)?;
                }
                KeyEvent::Pressed(Key::Left) | KeyEvent::Autorepeat(Key::Left) => {
                    self.add_volume(-1)?;
                }
                KeyEvent::Pressed(Key::Right) | KeyEvent::Autorepeat(Key::Right) => {
                    self.add_volume(1)?;
                }
                KeyEvent::Pressed(Key::Select) => {
                    self.rotate_display_profile()?;
                }
                KeyEvent::Released(Key::Power) => {
                    let game_info = GameInfo::load()?;
                    let name = match game_info.as_ref() {
                        Some(game_info) => game_info.name.as_str(),
                        None => "Allium",
                    };
                    let file_name = format!(
                        "{}-{}.png",
                        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S"),
                        name,
                    );
                    Command::new("screenshot")
                        .arg(ALLIUM_SD_ROOT.join("Screenshots").join(file_name))
                        .arg("--rumble")
                        .spawn()?
                        .wait()
                        .await?;
                }
                _ => {}
            }
        } else {
            match key_event {
                KeyEvent::Pressed(Key::VolDown) | KeyEvent::Autorepeat(Key::VolDown) => {
                    self.add_volume(-1)?
                }
                KeyEvent::Pressed(Key::VolUp) | KeyEvent::Autorepeat(Key::VolUp) => {
                    self.add_volume(1)?
                }
                KeyEvent::Autorepeat(Key::Power) if !self.keys[Key::Menu] => {
                    #[cfg(unix)]
                    self.handle_quit().await?;
                }
                KeyEvent::Released(Key::Power) if !self.keys[Key::Menu] => {
                    #[cfg(unix)]
                    match self.power_settings.power_button_action {
                        PowerButtonAction::Suspend => self.handle_suspend().await?,
                        PowerButtonAction::Shutdown => self.handle_quit().await?,
                        PowerButtonAction::Nothing => {}
                    }
                }
                KeyEvent::Pressed(Key::LidClose) =>
                {
                    #[cfg(unix)]
                    match self.power_settings.lid_close_action {
                        PowerButtonAction::Suspend => self.handle_suspend().await?,
                        PowerButtonAction::Shutdown => self.handle_quit().await?,
                        PowerButtonAction::Nothing => {}
                    }
                }
                KeyEvent::Released(Key::Menu) if self.is_menu_pressed_alone => {
                    if !self.menu_open
                        && self.is_ingame()
                        && self
                            .keys
                            .iter()
                            .all(|(k, pressed)| k == Key::Menu || !pressed)
                        && let Some(game_info) = GameInfo::load()?
                        && game_info.has_menu
                    {
                        let info = RetroArchCommand::GetInfo.send_recv().await?.map(|ret| {
                            let mut rets = ret.split_ascii_whitespace().skip(1);
                            let max_disk_slots = rets.next().map_or(0, |s| s.parse().unwrap_or(0));
                            let disk_slot = rets.next().map_or(0, |s| s.parse().unwrap_or(0));
                            let state_slot = rets.next().map(|s| s.parse().unwrap_or(0));
                            RetroArchInfo {
                                max_disk_slots,
                                disk_slot,
                                state_slot,
                            }
                        });

                        // Pause regardless of whether GetInfo answered. A timeout only means
                        // RetroArch was too busy to reply within 250ms, not that it is absent --
                        // and leaving it running would let it repaint straight over the menu,
                        // which looks exactly like the menu button having done nothing.
                        RetroArchCommand::Pause.send_or_log().await;
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

                        // The plate would keep re-flushing over the menu until its timeout
                        self.hide_osd();

                        if self.menu.tx.send(info).is_err() {
                            // Latching menu_open here would block every future menu press
                            error!("failed to send to menu thread");
                            RetroArchCommand::Unpause.send_or_log().await;
                        } else {
                            self.menu_open = true;
                        }
                    }
                    self.is_menu_pressed_alone = false;
                }
                _ => {}
            }
        }

        Ok(())
    }

    /// The configured charging boot action, falling back to charging silently where the board
    /// cannot actually power off (`shutdown` would reboot straight back into this branch).
    #[cfg(unix)]
    fn charging_boot_action(&self) -> ChargingBootAction {
        let action = self.power_settings.charging_boot_action;
        if action == ChargingBootAction::PowerOff && !DefaultPlatform::can_power_off() {
            warn!("this device cannot power off, charging silently instead");
            return ChargingBootAction::ChargeSilently;
        }
        action
    }

    /// Power back down after a charger-triggered boot, unless the user is actually trying to
    /// turn the device on. We can't tell those two apart here -- both look like "charging at
    /// startup" -- so wait briefly for a keypress first.
    ///
    /// This is only the fallback. `.tmp_update/updater` makes the same decision before the
    /// backlight is switched on, which is the only way to avoid lighting the panel at all; by the
    /// time this runs the boot has finished and the launcher is painting. Blank the screen so at
    /// least the remaining second is dark.
    #[cfg(unix)]
    async fn handle_charging_power_off(&mut self) -> Result<()> {
        info!("charging, powering off unless a key is pressed");

        self.hide_osd();

        #[allow(clippy::let_unit_value)]
        let ctx = self.platform.suspend()?;

        // `poll` only ever resolves on a real key or lid event, so anything at all here means a
        // person is at the device.
        let woken = tokio::select! {
            _ = self.platform.poll() => true,
            _ = tokio::time::sleep(CHARGE_POWER_OFF_GRACE) => false,
        };

        if woken {
            info!("key pressed while charging, booting normally");
            return self.platform.unsuspend(ctx);
        }

        self.platform.shutdown()?;

        // shutdown execs `poweroff`, so reaching here means it failed to replace us. Returning
        // would let the event loop carry on and, once alliumd exits, the updater's unconditional
        // reboot loop would boot us straight back into this -- so park instead.
        error!("poweroff did not take effect, holding to avoid a boot loop");
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        }
    }

    /// Park in the charge screen until the user presses Power or unplugs the cable. With
    /// `announce` the display lights up and says "Charging" first; without it the screen is never
    /// turned on at all.
    #[cfg(unix)]
    async fn handle_charging(&mut self, announce: bool) -> Result<()> {
        info!("charging...");

        self.hide_osd();

        signal(&self.main, Signal::SIGSTOP)?;

        if announce {
            Command::new("say")
                .arg(self.locale.t("charging"))
                .spawn()?
                .wait()
                .await?;

            tokio::time::sleep(std::time::Duration::from_secs(5)).await;

            Command::new("show").arg("-c").spawn()?.wait().await?;
        }

        #[allow(clippy::let_unit_value)]
        let ctx = self.platform.suspend()?;

        let mut battery = self.platform.battery()?;

        loop {
            tokio::select! {
                key_event = self.platform.poll() => {
                    if matches!(key_event, KeyEvent::Released(Key::Power)) {
                        break;
                    }
                }
                _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {
                    battery.update()?;
                    if !battery.charging() {
                        self.platform.shutdown()?;
                    }
                }
            }
        }

        signal(&self.main, Signal::SIGCONT)?;
        self.platform.unsuspend(ctx)
    }

    #[cfg(unix)]
    async fn handle_suspend(&mut self) -> Result<()> {
        info!("suspending...");
        self.hide_osd();
        #[allow(clippy::let_unit_value)]
        let ctx = self.platform.suspend()?;
        signal(&self.main, Signal::SIGSTOP)?;

        // A fixed point in time rather than a fresh sleep per pass: the loop re-enters on every key
        // event it discards, so a countdown built inside it restarts on any stray press -- hardly
        // visible at five minutes, very visible at ninety. Anything at or below zero means Never;
        // clamping a negative to zero would instead shut a hand-edited device down on the spot.
        let deadline = match self.power_settings.suspend_shutdown_minutes {
            minutes if minutes <= 0 => None,
            minutes => {
                Some(tokio::time::Instant::now() + std::time::Duration::new(minutes as u64 * 60, 0))
            }
        };
        let mut battery = self.platform.battery()?;

        loop {
            tokio::select! {
                key_event = self.platform.poll()=> {
                    if matches!(key_event, KeyEvent::Released(Key::Power)) || matches!(key_event, KeyEvent::Released(Key::LidClose)) {
                        self.keys[Key::Power] = false;
                        self.keys[Key::LidClose] = false;
                        break;
                    }
                }
                // The outer loop's battery check is parked while we sit in here, and this suspend
                // only blanks the panel -- the SoC still runs. Without this, a device suspended
                // near empty would draw idle current until the deadline and go flat, which loses
                // the game: a flat battery gets no clean shutdown.
                _ = tokio::time::sleep(BATTERY_UPDATE_INTERVAL) => {
                    battery.update()?;
                    if battery.percentage() <= BATTERY_SHUTDOWN_THRESHOLD && !battery.charging() {
                        warn!("battery is low while suspended, shutting down");
                        return self.wake_and_quit(ctx).await;
                    }
                }
                _ = sleep_until_wake(deadline) => {
                    info!("suspend timeout, shutting down");
                    return self.wake_and_quit(ctx).await;
                }
            }
        }

        info!("waking up from suspend...");
        signal(&self.main, Signal::SIGCONT)?;
        self.platform.unsuspend(ctx)
    }

    /// Undoes a suspend, then shuts down properly.
    ///
    /// The SIGCONT has to come first. `handle_quit` sends SIGTERM and gives the child five seconds,
    /// and a stopped process cannot run its handler -- so without this RetroArch never writes its
    /// auto-save and is SIGKILLed instead. That auto-save is the only thing saving the game;
    /// alliumd sends no save-state command on any shutdown path.
    #[cfg(unix)]
    async fn wake_and_quit(
        &mut self,
        ctx: <DefaultPlatform as Platform>::SuspendContext,
    ) -> Result<()> {
        signal(&self.main, Signal::SIGCONT)?;
        self.platform.unsuspend(ctx)?;
        self.handle_quit().await
    }

    #[cfg(unix)]
    async fn handle_quit(&mut self) -> Result<()> {
        if self.is_terminating {
            return Ok(());
        }

        debug!("terminating, saving state");

        self.hide_osd();
        self.state.time = Utc::now();
        self.state.save()?;

        if self.is_ingame() {
            self.update_play_time()?;
            // Menu thread will exit on its own when the process shuts down
        }

        terminate(&mut self.main).await?;

        self.is_terminating = true;

        Command::new("show").arg("--darken").spawn()?.wait().await?;
        Command::new("say")
            .arg(self.locale.t("powering-off"))
            .spawn()?
            .wait()
            .await?;
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;

        self.platform.shutdown()?;

        Ok(())
    }

    #[allow(unused)]
    /// Pauses before a respawn when the child keeps dying.
    ///
    /// A launcher that fails on startup would otherwise be restarted in a tight loop forever, the
    /// screen flashing, with nothing the user can do -- the only other way out is a reboot, which
    /// runs the same loop. A pause between attempts keeps the device answering the power button
    /// and leaves a log that can be read rather than one that scrolls past.
    async fn throttle_crash_loop(&mut self) {
        let now = Instant::now();
        self.child_exits
            .retain(|at| now.duration_since(*at) < CRASH_LOOP_WINDOW);
        self.child_exits.push_back(now);
        if self.child_exits.len() >= CRASH_LOOP_EXITS {
            warn!(
                "main process exited {} times in {:?}; waiting {:?} before starting it again",
                self.child_exits.len(),
                CRASH_LOOP_WINDOW,
                CRASH_LOOP_BACKOFF
            );
            tokio::time::sleep(CRASH_LOOP_BACKOFF).await;
        }
    }

    fn update_play_time(&self) -> Result<()> {
        if !self.is_ingame() {
            return Ok(());
        }

        let file = File::open(ALLIUM_GAME_INFO.as_path())?;
        let mut game_info: GameInfo = serde_json::from_reader(file)?;

        let duration = game_info.play_time();

        // As a sanity check, don't add play time if the game was played for more than 24 hours
        if duration > Duration::hours(24) {
            warn!("play time is too long, not adding to database");
            return Ok(());
        }

        let database = Database::new()?;
        database.add_play_time(game_info.path.as_path(), duration)?;

        // Record game session
        let start_time = game_info.start_time.timestamp();
        let end_time = Utc::now().timestamp();
        database.insert_game_session(
            game_info.path.as_path(),
            start_time,
            end_time,
            duration.num_seconds(),
        )?;

        Ok(())
    }

    fn is_ingame(&self) -> bool {
        Path::new(&*ALLIUM_GAME_INFO).exists()
    }

    /// Whether the foreground repaints the frame itself; apps exec over the launcher
    /// without writing game info, so `is_ingame` alone misses them
    fn foreground_repaints(&self) -> bool {
        let exe = self
            .main
            .id()
            .and_then(|pid| fs::canonicalize(format!("/proc/{pid}/exe")).ok());
        let launcher = fs::canonicalize(ALLIUM_LAUNCHER.as_path()).ok();
        match (exe, launcher) {
            (Some(exe), Some(launcher)) => exe != launcher,
            _ => self.is_ingame(),
        }
    }

    fn add_volume(&mut self, add: i32) -> Result<()> {
        info!("adding volume: {}", add);
        self.state.volume = (self.state.volume + add).clamp(0, MAX_VOLUME);
        // Draw first: set_volume spawns myctl, which costs tens of ms on this SoC
        self.show_osd(
            OsdKind::Volume,
            self.state.volume as f32 / MAX_VOLUME as f32,
        );
        self.platform.set_volume(self.state.volume)?;
        Ok(())
    }

    fn add_brightness(&mut self, add: i8) -> Result<()> {
        info!("adding brightness: {}", add);
        self.state.brightness =
            (self.state.brightness as i8 + add).clamp(0, MAX_BRIGHTNESS as i8) as u8;
        // Draw first, matching add_volume, so the two paths behave the same
        self.show_osd(
            OsdKind::Brightness,
            self.state.brightness as f32 / MAX_BRIGHTNESS as f32,
        );
        self.platform.set_brightness(self.state.brightness)?;
        Ok(())
    }

    /// Rotate to the next display profile. Driven through the display controller's colour
    /// registers rather than the framebuffer, so it applies to RetroArch's frames too.
    fn rotate_display_profile(&mut self) -> Result<()> {
        let mut settings = DisplaySettings::load()?;
        let active = settings.rotate();
        let name = settings.name_of(active);
        info!("display profile: {} ({})", active + 1, name);

        // Draw first, matching add_volume/add_brightness
        self.show_osd_label(OsdKind::DisplayProfile, name);

        self.platform
            .set_display_settings(&mut settings.active().effective())?;
        settings.save()?;
        Ok(())
    }

    fn show_osd(&mut self, kind: OsdKind, fraction: f32) {
        self.show_osd_content(kind, OsdContent::Bar(fraction));
    }

    /// Shows text instead of a bar, for something that has a name rather than a level.
    fn show_osd_label(&mut self, kind: OsdKind, label: String) {
        self.show_osd_content(kind, OsdContent::Label(label));
    }

    fn show_osd_content(&mut self, kind: OsdKind, content: OsdContent) {
        let repainting = self.foreground_repaints() && !self.menu_open;
        // Cosmetic only: a failed overlay must not take down the daemon
        if let Err(e) = self.osd.show(&mut self.platform, kind, content, repainting) {
            error!("failed to show OSD: {}", e);
        }
    }

    fn hide_osd(&mut self) {
        // Cosmetic only: must not block suspend or shutdown
        if let Err(e) = self.osd.hide() {
            error!("failed to hide OSD: {}", e);
        }
    }
}

#[allow(clippy::needless_pass_by_ref_mut)]
async fn terminate(child: &mut Child) -> Result<()> {
    #[cfg(unix)]
    signal(child, Signal::SIGTERM)?;
    #[cfg(not(unix))]
    child.kill().await?;

    #[cfg(unix)]
    if let Err(_e) = tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()).await {
        signal(child, Signal::SIGKILL)?;
    }
    Ok(())
}

#[cfg(unix)]
fn signal(child: &Child, signal: Signal) -> Result<()> {
    if let Some(pid) = child.id() {
        let pid = Pid::from_raw(pid as i32);
        kill(pid, signal)?;
    }
    Ok(())
}
