#![allow(unused)]
use std::env;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

use lazy_static::lazy_static;

use crate::geom::Size;

pub static ALLIUM_VERSION: LazyLock<String> = LazyLock::new(|| {
    let version_path = ALLIUM_BASE_DIR.join("version.txt");
    std::fs::read_to_string(&version_path)
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
});

#[cfg(feature = "miyoo")]
pub static ALLIUM_SD_ROOT: LazyLock<PathBuf> = LazyLock::new(|| {
    PathBuf::from(&env::var("ALLIUM_SD_ROOT").unwrap_or_else(|_| "/mnt/SDCARD/".to_string()))
});
#[cfg(feature = "simulator")]
pub static ALLIUM_SD_ROOT: LazyLock<PathBuf> = LazyLock::new(|| {
    env::var("ALLIUM_SD_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| env::current_dir().unwrap().join("simulator"))
});
#[cfg(not(any(feature = "miyoo", feature = "simulator")))]
pub static ALLIUM_SD_ROOT: LazyLock<PathBuf> =
    LazyLock::new(|| PathBuf::from(&env::var("ALLIUM_SD_ROOT").unwrap()));

lazy_static! {
    pub static ref ALLIUM_BASE_DIR: PathBuf = PathBuf::from(
        &env::var("ALLIUM_BASE_DIR").map_or_else(|_| ALLIUM_SD_ROOT.join(".allium"), PathBuf::from)
    );
    pub static ref ALLIUM_GAMES_DIR: PathBuf = PathBuf::from(
        &env::var("ALLIUM_GAMES_DIR").map_or_else(|_| ALLIUM_SD_ROOT.join("Roms"), PathBuf::from)
    );
    pub static ref ALLIUM_APPS_DIR: PathBuf = PathBuf::from(
        &env::var("ALLIUM_APPS_DIR").map_or_else(|_| ALLIUM_SD_ROOT.join("Apps"), PathBuf::from)
    );

    // Folders
    pub static ref ALLIUM_SCRIPTS_DIR: PathBuf = ALLIUM_BASE_DIR.join("scripts");
    pub static ref ALLIUM_TOOLS_DIR: PathBuf = ALLIUM_BASE_DIR.join("tools");
    pub static ref ALLIUM_FONTS_DIR: PathBuf = ALLIUM_BASE_DIR.join("fonts");
    pub static ref ALLIUM_THEMES_DIR: PathBuf = ALLIUM_SD_ROOT.join("Themes");
    pub static ref ALLIUM_LOCALES_DIR: PathBuf = ALLIUM_BASE_DIR.join("locales");
    pub static ref ALLIUM_IMAGES_DIR: PathBuf = ALLIUM_BASE_DIR.join("images");
    pub static ref ALLIUM_SCREENSHOTS_DIR: PathBuf = ALLIUM_SD_ROOT.join("Saves/CurrentProfile/screenshots");

    // Config
    pub static ref ALLIUM_CONFIG_CONSOLES: PathBuf = ALLIUM_BASE_DIR.join("config/consoles.toml");
    pub static ref ALLIUM_CONFIG_CORES: PathBuf = ALLIUM_BASE_DIR.join("config/cores.toml");

    // State
    pub static ref ALLIUMD_STATE: PathBuf = ALLIUM_BASE_DIR.join("state/alliumd.json");
    pub static ref ALLIUM_LAUNCHER_STATE: PathBuf =
        ALLIUM_BASE_DIR.join("state/allium-launcher.json");
    pub static ref ALLIUM_MENU_STATE: PathBuf =
        ALLIUM_BASE_DIR.join("state/allium-menu.json");
    pub static ref ALLIUM_GAME_INFO: PathBuf = ALLIUM_BASE_DIR.join("state/current_game");
    pub static ref ALLIUM_STYLESHEET: PathBuf = ALLIUM_BASE_DIR.join("state/stylesheet.json");
    pub static ref ALLIUM_DISPLAY_SETTINGS: PathBuf = ALLIUM_BASE_DIR.join("state/display.json");
    pub static ref ALLIUM_LOCALE_SETTINGS: PathBuf = ALLIUM_BASE_DIR.join("state/locale.json");
    pub static ref ALLIUM_POWER_SETTINGS: PathBuf = ALLIUM_BASE_DIR.join("state/power.json");
    pub static ref ALLIUM_WIFI_SETTINGS: PathBuf = ALLIUM_BASE_DIR.join("state/wifi.json");
    pub static ref ALLIUM_UPDATE_SETTINGS: PathBuf = ALLIUM_BASE_DIR.join("state/update.json");
    pub static ref ALLIUM_TIMEZONE: PathBuf = ALLIUM_BASE_DIR.join("state/timezone");
    pub static ref ALLIUM_THEME_STATE: PathBuf = ALLIUM_BASE_DIR.join("state/theme");

    // Database
    pub static ref ALLIUM_DATABASE: PathBuf = env::var("ALLIUM_DATABASE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| ALLIUM_SD_ROOT.join("Saves/CurrentProfile/allium.db"));

    // Binaries & Scripts
    pub static ref ALLIUM_LAUNCHER: PathBuf = ALLIUM_BASE_DIR.join("bin/allium-launcher");
    pub static ref ALLIUM_MENU: PathBuf = ALLIUM_BASE_DIR.join("bin/allium-menu");
    pub static ref ALLIUM_RETROARCH: PathBuf = ALLIUM_BASE_DIR.join("cores/retroarch/launch.sh");
}

// Styles
pub const IMAGE_WIDTH: u32 = 250;
pub const SAVE_STATE_IMAGE_WIDTH: u32 = 333;

/// Volume scale is 0..=MAX_VOLUME, mapped to the hardware dB curve at the platform layer.
pub const MAX_VOLUME: i32 = 20;
/// Brightness scale is 0..=MAX_BRIGHTNESS percent.
pub const MAX_BRIGHTNESS: u8 = 100;

/// Night mode dims the panel and cuts its green and blue channels, leaving red untouched, so the
/// screen goes warm and dark for playing without light. These scale the user's base display
/// settings; tune them here.
pub const NIGHT_MODE_LUMINANCE_SCALE: f32 = 0.55;
pub const NIGHT_MODE_GREEN_SCALE: f32 = 0.72;
pub const NIGHT_MODE_BLUE_SCALE: f32 = 0.35;
/// Floor for the scaled green/blue channels. The platform flattens all three channels back to
/// neutral grey when *every* one of them falls below 15, which would cancel the tint entirely.
/// Holding green and blue at 15 makes that condition unreachable whatever the user's base red is.
pub const NIGHT_MODE_CHANNEL_FLOOR: u8 = 15;

/// Set by the in-game menu to ask for the running game to be started again rather than dropped
/// back to the launcher, so a setting RetroArch only reads at content load takes effect.
///
/// On tmpfs, like `/tmp/stay_awake`, so it cannot survive a reboot and strand the device
/// relaunching. The daemon consumes it by deleting it.
pub const RELAUNCH_MARKER: &str = "/tmp/allium_relaunch";

/// How long the daemon waits for a keypress before powering back off after a charger-triggered
/// boot, so pressing Power while the cable is attached still turns the device on.
pub const CHARGE_POWER_OFF_GRACE: Duration = Duration::from_millis(1500);

/// After the battery level drops below this threshold, the charging LED will blink at 0.5Hz.
pub const BATTERY_WARNING_THRESHOLD: i32 = 15;
/// After the battery level drops below this threshold, the device will shut down.
pub const BATTERY_SHUTDOWN_THRESHOLD: i32 = 5;

/// The interval at which the battery level is updated.
pub const BATTERY_UPDATE_INTERVAL: Duration = Duration::from_secs(10);

/// The interval at which the WiFi status is updated.
pub const WIFI_UPDATE_INTERVAL: Duration = Duration::from_secs(10);

/// The interval at which the clock is updated.
pub const CLOCK_UPDATE_INTERVAL: Duration = Duration::from_secs(60);

/// The UI frame period; a launcher redraw flushes the whole screen.
pub const UI_FRAME_INTERVAL: Duration = Duration::from_micros(166_667);

/// The number of items to jump when pressing left/right in a listing.
pub const LISTING_JUMP_SIZE: i32 = 5;

/// If a key autorepeat is received after this duration, it will be ignored.
pub const MAXIMUM_FRAME_TIME: Duration = Duration::from_millis(100);

/// Maximum number of recent games to retrieve from the database.
pub const RECENT_GAMES_LIMIT: i64 = 100;

/// RetroArch network command interface.
pub const RETROARCH_UDP_SOCKET: &str = "127.0.0.1:55355";
