use std::{
    fs::{self, File},
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use log::{debug, warn};
use serde::{Deserialize, Serialize};

use crate::constants::{ALLIUM_GAME_INFO, ALLIUM_GAMES_DIR, ALLIUM_SCRIPTS_DIR};
use crate::performance::PerformanceMode;

#[derive(Debug, Serialize, Deserialize)]
/// Information about a game. Used to restore a game after a restart, and to calculate playtime.
pub struct GameInfo {
    /// Display name of the game.
    pub name: String,
    /// Path to the game rom. This is used to generate the screenshot name.
    pub path: PathBuf,
    /// Core used to run the game. This is used to generate the screenshot name.
    pub core: String,
    /// Command to run the core.
    pub command: String,
    /// Arguments to pass to the core to run the game.
    pub args: Vec<String>,
    /// Do we enable the menu? Currently only enabled if RetroArch is used.
    pub has_menu: bool,
    /// Whether swap should be enabled.
    pub needs_swap: bool,
    /// The CPU governor preset to run this game at, already resolved against the global default.
    ///
    /// Carried here rather than re-read from the database because alliumd rebuilds the child from
    /// this file alone when resuming, and opening SQLite on that path would cost time on a launch
    /// that is already the slow part. `serde(default)` is load-bearing: without it, a state file
    /// written by an older build fails to parse and `load` deletes it, dropping the game in
    /// progress on the first boot after an update.
    #[serde(default)]
    pub performance_mode: PerformanceMode,
    /// Path to the image.
    pub image: Option<PathBuf>,
    /// Paths to the guide text files.
    pub guides: Vec<PathBuf>,
    /// Start time. Used to measure playtime.
    pub start_time: DateTime<Utc>,
}

impl Default for GameInfo {
    fn default() -> Self {
        Self {
            name: String::new(),
            path: PathBuf::new(),
            core: String::new(),
            command: String::new(),
            args: Vec::new(),
            has_menu: false,
            needs_swap: false,
            performance_mode: PerformanceMode::default(),
            image: None,
            guides: Vec::new(),
            start_time: Utc::now(),
        }
    }
}

impl GameInfo {
    /// Create a new GameInfo object.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        name: String,
        path: PathBuf,
        core: String,
        image: Option<PathBuf>,
        command: String,
        args: Vec<String>,
        has_menu: bool,
        needs_swap: bool,
    ) -> Self {
        let guides = find_guides(&path);

        Self {
            name,
            path,
            core,
            command,
            args,
            has_menu,
            // `new` already takes eight positional arguments; the caller assigns this straight
            // after, once it has resolved the game's mode against the global default.
            performance_mode: PerformanceMode::default(),
            needs_swap,
            image,
            guides,
            start_time: Utc::now(),
        }
    }

    /// Loads the current game info from file, if exists.
    pub fn load() -> Result<Option<Self>> {
        Ok(if ALLIUM_GAME_INFO.exists() {
            let file = File::open(ALLIUM_GAME_INFO.as_path())?;
            let Ok(game_info) = serde_json::from_reader::<_, Self>(file) else {
                fs::remove_file(ALLIUM_GAME_INFO.as_path())?;
                return Ok(None);
            };
            if game_info.needs_swap() {
                debug!("enabling swap");
                Command::new(ALLIUM_SCRIPTS_DIR.join("swap-on.sh"))
                    .spawn()?
                    .wait()?;
            }
            Some(game_info)
        } else {
            None
        })
    }

    /// Saves the current game info to file.
    pub fn save(&self) -> Result<()> {
        let file = File::create(ALLIUM_GAME_INFO.as_path())?;
        serde_json::to_writer(file, self)?;
        Ok(())
    }

    /// Deletes the current game info file.
    pub fn delete() -> Result<()> {
        if ALLIUM_GAME_INFO.exists() {
            fs::remove_file(ALLIUM_GAME_INFO.as_path())?;
        }
        Ok(())
    }

    /// Returns a command to run the game.
    pub fn command(self) -> Command {
        let mut command = Command::new(self.command);
        command.args(self.args);
        command
    }

    /// How long the game has been running.
    pub fn play_time(&self) -> Duration {
        Utc::now().signed_duration_since(self.start_time)
    }

    /// Whether swap should be enabled.
    pub fn needs_swap(&self) -> bool {
        self.needs_swap
    }

    /// Records a new performance mode for the game currently running.
    ///
    /// Deliberately does not round-trip through [`GameInfo::load`], which spawns `swap-on.sh` and
    /// deletes the state file on a parse failure -- neither is wanted from the in-game menu with
    /// a game running. Without this, changing the mode mid-game would be undone by the next
    /// resume, which re-applies whatever was resolved at launch.
    pub fn store_performance_mode(mode: PerformanceMode) -> Result<()> {
        let Ok(file) = File::open(ALLIUM_GAME_INFO.as_path()) else {
            debug!("no game info to update, not saving the performance mode");
            return Ok(());
        };
        let Ok(mut game_info) = serde_json::from_reader::<_, Self>(file) else {
            warn!("could not read game info, not saving the performance mode");
            return Ok(());
        };

        game_info.performance_mode = mode;
        game_info.save()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn older_state_files_still_parse() {
        // `load` deletes the state file on a parse failure, which would drop the game in progress
        // on the first boot after an update. A file written before `performance_mode` existed has
        // to keep deserializing.
        let legacy = r#"{
            "name": "Game One",
            "path": "Roms/GBA/Game One.gba",
            "core": "mgba",
            "command": "/mnt/SDCARD/.allium/cores/retroarch/launch.sh",
            "args": ["mgba", "Roms/GBA/Game One.gba"],
            "has_menu": true,
            "needs_swap": false,
            "image": null,
            "guides": [],
            "start_time": "2026-01-01T00:00:00Z"
        }"#;

        let game_info: GameInfo = serde_json::from_str(legacy).unwrap();
        assert_eq!(game_info.name, "Game One");
        assert_eq!(game_info.performance_mode, PerformanceMode::System);
    }
}

/// Searches for all guide paths and returns them
pub fn find_guides(path: &Path) -> Vec<PathBuf> {
    // Search for Guides folder upwards, recursively
    let mut parent = path.to_path_buf();
    let mut guides = Vec::new();
    'guide: while parent.pop() {
        let mut guide_path = parent.join("Guides");
        if guide_path.is_dir() {
            guide_path.extend(path.strip_prefix(&parent).unwrap());

            debug!("Checking guide path: {:?}", guide_path);
            guide_path.set_extension("");
            if guide_path.is_dir() {
                debug!("Found guide directory: {:?}", guide_path);
                // Collect all guides in the directory
                if let Ok(entries) = fs::read_dir(&guide_path) {
                    let mut found_guides: Vec<PathBuf> = entries
                        .filter_map(|entry| {
                            let entry = entry.ok()?;
                            let path = entry.path();
                            if path.is_file() { Some(path) } else { None }
                        })
                        .collect();
                    // Sort guides alphabetically for consistent ordering
                    found_guides.sort();
                    if !found_guides.is_empty() {
                        guides = found_guides;
                        break 'guide;
                    }
                }
            }

            const GUIDE_EXTENSIONS: [&str; 1] = ["txt"];
            for ext in &GUIDE_EXTENSIONS {
                guide_path.set_extension(ext);
                debug!("Checking guide file: {:?}", guide_path);
                if guide_path.is_file() {
                    guides.push(guide_path.clone());
                    break 'guide;
                }
            }
        }
        if parent.to_str() == ALLIUM_GAMES_DIR.to_str() {
            break;
        }
    }
    guides
}
