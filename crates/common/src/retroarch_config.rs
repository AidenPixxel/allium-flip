//! Reading and writing RetroArch's per-core, per-console and per-game override files.
//!
//! RetroArch already has a three-tier override system and it is already switched on in this
//! distribution -- `auto_overrides_enable`, `auto_remaps_enable` and `game_specific_options` are
//! all true, and `config_save_on_exit` is false, so it will not clobber what Allium writes. Later
//! tiers win: core, then the ROM's folder, then the individual ROM.
//!
//! Nothing here takes effect on a running game. RetroArch reads overrides once, when it loads
//! content, so every change applies the next time the game is launched.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use log::{debug, warn};
use serde::{Deserialize, Serialize};
use strum::FromRepr;

use crate::constants::ALLIUM_SD_ROOT;

/// Which of RetroArch's override tiers a change is written to.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, FromRepr, Default)]
pub enum OverrideScope {
    /// This ROM alone. Beats both tiers below it.
    #[default]
    Game,
    /// Every ROM in the same folder -- which is how a console is expressed here, since
    /// `Roms/<console>` is exactly RetroArch's "content directory".
    Console,
    /// Every game run by this core, whichever console it belongs to.
    Core,
}

/// Where RetroArch keeps its overrides, per `rgui_config_directory`.
fn config_dir() -> PathBuf {
    ALLIUM_SD_ROOT.join("Saves/CurrentProfile/config")
}

/// Where RetroArch keeps input remaps, per `input_remapping_directory`.
fn remap_dir() -> PathBuf {
    config_dir().join("remaps")
}

/// The core's own name for itself, which is what RetroArch names override directories after.
///
/// Deliberately read from the shipped `.info` file rather than taken from `cores.toml`: the two
/// disagree for PCSX-ReARMed, PrBoom, the Beetle cores and the FBA variants, and writing to a
/// directory built from the wrong one produces files RetroArch silently never reads.
pub fn core_name(libretro_core: &str) -> Option<String> {
    let path = ALLIUM_SD_ROOT
        .join("RetroArch/.retroarch/cores")
        .join(format!("{libretro_core}_libretro.info"));

    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) => {
            warn!("could not read {}: {err}", path.display());
            return None;
        }
    };

    let name = parse(&text).remove("corename").filter(|n| !n.is_empty());
    if name.is_none() {
        warn!("no corename in {}", path.display());
    }
    name
}

/// The override file a scope writes to, given the core's name and the ROM.
///
/// `None` for [`OverrideScope::Console`] when the ROM sits at the top of the Roms directory with
/// no folder to name -- there is no content directory for RetroArch to match against.
pub fn override_path(scope: OverrideScope, core_name: &str, rom: &Path) -> Option<PathBuf> {
    Some(
        config_dir()
            .join(core_name)
            .join(scope_file_name(scope, core_name, rom)? + ".cfg"),
    )
}

/// The remap file a scope writes to. Same tiers, different directory and extension.
pub fn remap_path(scope: OverrideScope, core_name: &str, rom: &Path) -> Option<PathBuf> {
    Some(
        remap_dir()
            .join(core_name)
            .join(scope_file_name(scope, core_name, rom)? + ".rmp"),
    )
}

/// The stem RetroArch matches a tier on: the ROM, its folder, or the core.
fn scope_file_name(scope: OverrideScope, core_name: &str, rom: &Path) -> Option<String> {
    match scope {
        OverrideScope::Game => Some(rom.file_stem()?.to_string_lossy().into_owned()),
        OverrideScope::Console => Some(rom.parent()?.file_name()?.to_string_lossy().into_owned()),
        OverrideScope::Core => Some(core_name.to_owned()),
    }
}

/// Parses RetroArch's `key = "value"` format.
///
/// Shared by its `.cfg`, `.opt`, `.rmp` and `.info` files, which all use it. Unquoted values are
/// accepted too, and anything unparseable is skipped rather than failing the whole file -- these
/// are user-editable files and one bad line should not discard the rest.
pub fn parse(text: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        let value = value.trim().trim_matches('"');
        map.insert(key.to_owned(), value.to_owned());
    }

    map
}

/// Renders the map back out. Sorted, because `BTreeMap` keeps it that way and a stable order makes
/// the files diffable and the tests meaningful.
pub fn render(map: &BTreeMap<String, String>) -> String {
    let mut out = String::new();
    for (key, value) in map {
        out.push_str(key);
        out.push_str(" = \"");
        out.push_str(value);
        out.push_str("\"\n");
    }
    out
}

/// Applies `changes` to the file at `path`, leaving every other key alone.
///
/// A `None` value **removes** the key. That is what makes the tiers work: a game-level file must
/// only carry the settings actually set for that game, or it would shadow the console and core
/// files for everything else. A change set that empties the file deletes it, for the same reason.
///
/// Merging rather than rewriting also matters because the shipped per-core overrides already carry
/// keys Allium knows nothing about -- `video_dingux_ipu_keep_aspect`, `audio_driver`,
/// `audio_latency` -- and dropping those would change how the core runs.
pub fn apply(path: &Path, changes: &[(&str, Option<String>)]) -> Result<()> {
    let existing = fs::read_to_string(path).unwrap_or_default();
    let mut map = parse(&existing);

    for (key, value) in changes {
        match value {
            Some(value) => {
                map.insert((*key).to_owned(), value.clone());
            }
            None => {
                map.remove(*key);
            }
        }
    }

    if map.is_empty() {
        if path.exists() {
            debug!("no overrides left, removing {}", path.display());
            fs::remove_file(path)
                .with_context(|| format!("failed to remove {}", path.display()))?;
        }
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let rendered = render(&map);
    if rendered == existing {
        return Ok(());
    }

    debug!("writing {} override keys to {}", map.len(), path.display());
    fs::write(path, rendered).with_context(|| format!("failed to write {}", path.display()))
}

/// Reads a file in this format, or an empty map when it does not exist -- which is the normal case
/// for a scope nothing has been set at yet.
pub fn read(path: &Path) -> BTreeMap<String, String> {
    fs::read_to_string(path)
        .map(|text| parse(&text))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch directory for one test, named after it so parallel tests cannot collide.
    ///
    /// Hand-rolled because `tempfile` is not in the workspace, and adding a dependency would need
    /// a lockfile update that cannot be produced on the machine this was written on.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("allium-retroarch-config-{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn parses_the_retroarch_format() {
        let text = r#"
# a comment
aspect_ratio_index = "23"
video_dingux_ipu_keep_aspect = "true"

unquoted = 4
  indented = "yes"
"#;
        let map = parse(text);
        assert_eq!(
            map.get("aspect_ratio_index").map(String::as_str),
            Some("23")
        );
        assert_eq!(
            map.get("video_dingux_ipu_keep_aspect").map(String::as_str),
            Some("true")
        );
        assert_eq!(map.get("unquoted").map(String::as_str), Some("4"));
        assert_eq!(map.get("indented").map(String::as_str), Some("yes"));
        // The comment and the blank lines contribute nothing
        assert_eq!(map.len(), 4);
    }

    #[test]
    fn skips_junk_without_discarding_the_file() {
        // These files are user-editable, so one bad line must not lose the good ones
        let map = parse("good = \"1\"\nnonsense\n= \"no key\"\nalso_good = \"2\"\n");
        assert_eq!(map.len(), 2);
        assert!(map.contains_key("good"));
        assert!(map.contains_key("also_good"));
    }

    #[test]
    fn round_trips() {
        let text = "b = \"2\"\na = \"1\"\n";
        // Sorted on the way out, so the input order is not preserved -- only the content
        assert_eq!(render(&parse(text)), "a = \"1\"\nb = \"2\"\n");
    }

    #[test]
    fn info_files_use_the_same_format() {
        let info = r#"# Software Information
display_name = "Nintendo - Game Boy Advance (mGBA)"
corename = "mGBA"
"#;
        assert_eq!(
            parse(info).get("corename").map(String::as_str),
            Some("mGBA")
        );
    }

    #[test]
    fn each_scope_names_a_different_file() {
        let rom = Path::new("/mnt/SDCARD/Roms/GBA/Golden Sun.gba");

        assert_eq!(
            scope_file_name(OverrideScope::Game, "mGBA", rom).unwrap(),
            "Golden Sun"
        );
        assert_eq!(
            scope_file_name(OverrideScope::Console, "mGBA", rom).unwrap(),
            "GBA"
        );
        assert_eq!(
            scope_file_name(OverrideScope::Core, "mGBA", rom).unwrap(),
            "mGBA"
        );
    }

    #[test]
    fn scope_file_name_handles_awkward_roms() {
        // Multiple extensions: RetroArch matches on the stem, so only the last one comes off
        assert_eq!(
            scope_file_name(
                OverrideScope::Game,
                "mGBA",
                Path::new("/Roms/GBA/Game (USA).gba.zip")
            )
            .unwrap(),
            "Game (USA).gba"
        );

        // A ROM sitting at the root has no folder for RetroArch to match a content-dir override
        // against, so there is nothing to write and the caller must hide the scope
        assert_eq!(
            scope_file_name(OverrideScope::Console, "mGBA", Path::new("Game.gba")),
            None
        );
    }

    #[test]
    fn apply_leaves_keys_it_does_not_manage_alone() {
        let dir = scratch("keeps-unmanaged");
        let path = dir.join("mGBA.cfg");

        // Stand in for a shipped per-core override
        fs::write(
            &path,
            "audio_driver = \"sdl\"\nfastforward_frameskip = \"true\"\n",
        )
        .unwrap();

        apply(&path, &[("fps_show", Some("true".to_owned()))]).unwrap();

        let map = read(&path);
        assert_eq!(map.get("audio_driver").map(String::as_str), Some("sdl"));
        assert_eq!(
            map.get("fastforward_frameskip").map(String::as_str),
            Some("true")
        );
        assert_eq!(map.get("fps_show").map(String::as_str), Some("true"));
    }

    #[test]
    fn apply_removes_a_key_set_back_to_none() {
        let dir = scratch("removes-key");
        let path = dir.join("mGBA.cfg");
        fs::write(&path, "audio_driver = \"sdl\"\nfps_show = \"true\"\n").unwrap();

        apply(&path, &[("fps_show", None)]).unwrap();

        let map = read(&path);
        assert!(!map.contains_key("fps_show"));
        // Unsetting one key must not take the rest of the file with it
        assert_eq!(map.get("audio_driver").map(String::as_str), Some("sdl"));
    }

    #[test]
    fn apply_deletes_a_file_with_nothing_left_in_it() {
        let dir = scratch("deletes-empty");
        let path = dir.join("Golden Sun.cfg");
        fs::write(&path, "fps_show = \"true\"\n").unwrap();

        apply(&path, &[("fps_show", None)]).unwrap();

        // A game-level file that still existed but was empty would be harmless, but leaving it
        // there accumulates clutter for every game the user ever opened this screen on
        assert!(!path.exists());
    }

    #[test]
    fn apply_creates_the_directory() {
        let dir = scratch("creates-dir");
        let path = dir.join("mGBA").join("Golden Sun.cfg");

        apply(&path, &[("fps_show", Some("true".to_owned()))]).unwrap();

        assert!(path.exists());
        assert_eq!(
            read(&path).get("fps_show").map(String::as_str),
            Some("true")
        );
    }

    #[test]
    fn apply_on_a_missing_file_with_nothing_to_set_writes_nothing() {
        let dir = scratch("nothing-to-set");
        let path = dir.join("mGBA").join("Golden Sun.cfg");

        apply(&path, &[("fps_show", None)]).unwrap();

        assert!(!path.exists());
        assert!(!path.parent().unwrap().exists());
    }
}
