//! The RetroArch settings Allium exposes, as data rather than code.
//!
//! Each entry names the keys it owns, the choices offered for it, and which file the choice is
//! written to. One screen renders any of these tables, so adding a setting is a table entry rather
//! than a new view.
//!
//! Button numbers are SDL joystick indices for this hardware, and RetroPad ids for remaps. Both
//! are taken from the config Allium used to ship before it switched to RetroArch's own defaults
//! (`git show 8b1950b^:assets/root/RetroArch/.retroarch/retroarch.cfg`), which is the only
//! authority in the repo for what physical button each number is.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::Result;

use crate::retroarch_config::{self, OverrideScope};

/// The override files belonging to one game.
pub struct Overrides {
    core_name: String,
    rom: PathBuf,
}

impl Overrides {
    /// `None` when the core's name cannot be resolved. That is the only thing that makes the paths
    /// unknowable -- without it there is no directory to write into, and guessing one would
    /// produce files RetroArch never reads.
    pub fn new(libretro_core: &str, rom: PathBuf) -> Option<Self> {
        Some(Self {
            core_name: retroarch_config::core_name(libretro_core)?,
            rom,
        })
    }

    pub fn core_name(&self) -> &str {
        &self.core_name
    }

    fn path(&self, scope: OverrideScope, target: Target) -> Option<PathBuf> {
        match target {
            Target::Cfg => retroarch_config::override_path(scope, &self.core_name, &self.rom),
            Target::Remap => retroarch_config::remap_path(scope, &self.core_name, &self.rom),
        }
    }

    /// Whether this scope names a file for this game. Only `Console` can fail to, for a ROM
    /// sitting at the top of the Roms directory with no folder for RetroArch to match on.
    pub fn supports(&self, scope: OverrideScope) -> bool {
        self.path(scope, Target::Cfg).is_some()
    }

    /// What is set at this scope, and only at this scope.
    ///
    /// Deliberately not the merge of all three tiers: the screen edits one tier at a time, and
    /// showing a value inherited from a broader scope would make it look as though it had been
    /// chosen here -- so changing scope and changing nothing else would silently pin it.
    pub fn read(&self, scope: OverrideScope, target: Target) -> BTreeMap<String, String> {
        self.path(scope, target)
            .map(|path| retroarch_config::read(&path))
            .unwrap_or_default()
    }

    pub fn apply(&self, scope: OverrideScope, setting: &Setting, choice: usize) -> Result<()> {
        let Some(path) = self.path(scope, setting.target) else {
            return Ok(());
        };
        retroarch_config::apply(&path, &setting.changes(choice))
    }
}

/// Which file a choice is written to. Pad remaps live in their own directory with their own
/// extension, and their key syntax is the inverse of a `.cfg`'s.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Target {
    Cfg,
    Remap,
}

/// One option offered for a setting.
pub struct Choice {
    /// Locale key for what the user sees.
    pub label: &'static str,
    /// Keys and values this choice writes. Empty means "Default": every key the setting owns is
    /// removed, so whatever the tier below says applies instead.
    pub sets: &'static [(&'static str, &'static str)],
}

/// One row on an override screen.
pub struct Setting {
    /// Locale key for the row label.
    pub label: &'static str,
    /// Locale key describing what the row does, shown under the list.
    pub description: &'static str,
    /// Every key this setting owns. Listed separately from the choices so Default can clear all of
    /// them even when no single choice mentions them all.
    pub keys: &'static [&'static str],
    pub target: Target,
    /// Index 0 is Default by convention -- the one with no keys set.
    pub choices: &'static [Choice],
}

impl Setting {
    /// Which choice the given file contents currently match.
    ///
    /// A choice matches when every key it sets is present with that value. Falls back to 0
    /// (Default), which is also the answer when the keys are absent -- exactly what an unset
    /// setting at this tier looks like.
    ///
    /// Index 0 is skipped rather than tested: Default sets nothing, and `all` over an empty
    /// iterator is true, so it would match every file and shadow every real choice.
    pub fn current(&self, map: &BTreeMap<String, String>) -> usize {
        self.choices
            .iter()
            .enumerate()
            .skip(1)
            .find(|(_, choice)| {
                choice
                    .sets
                    .iter()
                    .all(|(key, value)| map.get(*key).map(String::as_str) == Some(*value))
            })
            .map(|(index, _)| index)
            .unwrap_or(0)
    }

    /// The change set for picking `choice`: everything this setting owns cleared, then the chosen
    /// choice's keys written over the top. Clearing first is what makes switching between choices
    /// that own different keys leave nothing behind.
    pub fn changes(&self, choice: usize) -> Vec<(&'static str, Option<String>)> {
        let sets = self.choices.get(choice).map_or(&[][..], |c| c.sets);

        self.keys
            .iter()
            .map(|key| {
                let value = sets
                    .iter()
                    .find(|(k, _)| k == key)
                    .map(|(_, value)| (*value).to_owned());
                (*key, value)
            })
            .collect()
    }
}

/// Default, offered as the first choice on every setting.
const DEFAULT: Choice = Choice {
    label: "override-default",
    sets: &[],
};

macro_rules! on_off {
    ($key:literal) => {
        &[
            DEFAULT,
            Choice {
                label: "override-on",
                sets: &[($key, "true")],
            },
            Choice {
                label: "override-off",
                sets: &[($key, "false")],
            },
        ]
    };
}

/// The Options screen.
pub const OPTIONS: &[Setting] = &[
    Setting {
        label: "override-aspect-ratio",
        description: "override-desc-aspect-ratio",
        // Only the dingux IPU key. The generic `aspect_ratio_index` is what the shipped per-core
        // overrides set, but this build scales through the hardware IPU and the index's meaning
        // there is not established anywhere in the repo -- so it is left alone rather than guessed.
        keys: &["video_dingux_ipu_keep_aspect"],
        target: Target::Cfg,
        choices: &[
            DEFAULT,
            Choice {
                label: "override-aspect-ratio-keep",
                sets: &[("video_dingux_ipu_keep_aspect", "true")],
            },
            Choice {
                label: "override-aspect-ratio-full",
                sets: &[("video_dingux_ipu_keep_aspect", "false")],
            },
        ],
    },
    Setting {
        label: "override-auto-save",
        description: "override-desc-auto-save",
        keys: &["savestate_auto_save"],
        target: Target::Cfg,
        choices: on_off!("savestate_auto_save"),
    },
    Setting {
        label: "override-auto-load",
        description: "override-desc-auto-load",
        keys: &["savestate_auto_load"],
        target: Target::Cfg,
        choices: on_off!("savestate_auto_load"),
    },
    Setting {
        label: "override-fast-forward-ratio",
        description: "override-desc-fast-forward-ratio",
        keys: &["fastforward_ratio"],
        target: Target::Cfg,
        choices: &[
            DEFAULT,
            Choice {
                label: "override-ratio-2x",
                sets: &[("fastforward_ratio", "2.0")],
            },
            Choice {
                label: "override-ratio-4x",
                sets: &[("fastforward_ratio", "4.0")],
            },
            Choice {
                label: "override-ratio-unlimited",
                sets: &[("fastforward_ratio", "0.0")],
            },
        ],
    },
    Setting {
        label: "override-show-fps",
        description: "override-desc-show-fps",
        keys: &["fps_show"],
        target: Target::Cfg,
        choices: on_off!("fps_show"),
    },
    Setting {
        label: "override-rewind",
        description: "override-desc-rewind",
        keys: &["rewind_enable"],
        target: Target::Cfg,
        choices: on_off!("rewind_enable"),
    },
];

/// Builds a hotkey row's choices: Off, then every button.
///
/// The numbers are SDL indices for this hardware: A 8, B 0, X 9, Y 1, L 10, R 11, L2 12, R2 13,
/// Start 3, Select 2, Menu 14. The D-pad (4-7) is deliberately not offered — binding a hotkey to a
/// direction would make it unusable in every game.
macro_rules! hotkey {
    ($key:literal) => {
        &[
            DEFAULT,
            Choice {
                label: "override-off",
                sets: &[($key, "nul")],
            },
            Choice {
                label: "override-button-a",
                sets: &[($key, "8")],
            },
            Choice {
                label: "override-button-b",
                sets: &[($key, "0")],
            },
            Choice {
                label: "override-button-x",
                sets: &[($key, "9")],
            },
            Choice {
                label: "override-button-y",
                sets: &[($key, "1")],
            },
            Choice {
                label: "override-button-l",
                sets: &[($key, "10")],
            },
            Choice {
                label: "override-button-r",
                sets: &[($key, "11")],
            },
            Choice {
                label: "override-button-l2",
                sets: &[($key, "12")],
            },
            Choice {
                label: "override-button-r2",
                sets: &[($key, "13")],
            },
            Choice {
                label: "override-button-start",
                sets: &[($key, "3")],
            },
            Choice {
                label: "override-button-select",
                sets: &[($key, "2")],
            },
        ]
    };
}

/// RetroPad ids a physical button can be made to produce, for `.rmp` remaps.
macro_rules! remap {
    ($key:literal) => {
        &[
            DEFAULT,
            Choice {
                label: "override-off",
                sets: &[($key, "-1")],
            },
            Choice {
                label: "override-button-a",
                sets: &[($key, "8")],
            },
            Choice {
                label: "override-button-b",
                sets: &[($key, "0")],
            },
            Choice {
                label: "override-button-x",
                sets: &[($key, "9")],
            },
            Choice {
                label: "override-button-y",
                sets: &[($key, "1")],
            },
            Choice {
                label: "override-button-l",
                sets: &[($key, "10")],
            },
            Choice {
                label: "override-button-r",
                sets: &[($key, "11")],
            },
            Choice {
                label: "override-button-l2",
                sets: &[($key, "12")],
            },
            Choice {
                label: "override-button-r2",
                sets: &[($key, "13")],
            },
            Choice {
                label: "override-button-start",
                sets: &[($key, "3")],
            },
            Choice {
                label: "override-button-select",
                sets: &[($key, "2")],
            },
        ]
    };
}

/// The Controls screen: the hotkey gate, the hotkeys themselves, then pad remapping.
pub const CONTROLS: &[Setting] = &[
    Setting {
        // RetroArch has one global hotkey gate, not one per hotkey. With it set, every hotkey
        // below needs it held; with it Off, each fires on its own button -- and that button stops
        // reaching the game, which is why L2 and R2 are the sensible ones to use.
        label: "override-hotkey-gate",
        description: "override-desc-hotkey-gate",
        keys: &["input_enable_hotkey_btn"],
        target: Target::Cfg,
        choices: &[
            DEFAULT,
            Choice {
                label: "override-off",
                sets: &[("input_enable_hotkey_btn", "nul")],
            },
            Choice {
                label: "override-button-menu",
                sets: &[("input_enable_hotkey_btn", "14")],
            },
            Choice {
                label: "override-button-l2",
                sets: &[("input_enable_hotkey_btn", "12")],
            },
            Choice {
                label: "override-button-r2",
                sets: &[("input_enable_hotkey_btn", "13")],
            },
        ],
    },
    Setting {
        label: "override-hotkey-fast-forward",
        description: "override-desc-hotkey",
        keys: &["input_toggle_fast_forward_btn"],
        target: Target::Cfg,
        choices: hotkey!("input_toggle_fast_forward_btn"),
    },
    Setting {
        label: "override-hotkey-rewind",
        description: "override-desc-hotkey",
        keys: &["input_rewind_btn"],
        target: Target::Cfg,
        choices: hotkey!("input_rewind_btn"),
    },
    Setting {
        label: "override-hotkey-save-state",
        description: "override-desc-hotkey",
        keys: &["input_save_state_btn"],
        target: Target::Cfg,
        choices: hotkey!("input_save_state_btn"),
    },
    Setting {
        label: "override-hotkey-load-state",
        description: "override-desc-hotkey",
        keys: &["input_load_state_btn"],
        target: Target::Cfg,
        choices: hotkey!("input_load_state_btn"),
    },
    Setting {
        label: "override-hotkey-screenshot",
        description: "override-desc-hotkey",
        keys: &["input_screenshot_btn"],
        target: Target::Cfg,
        choices: hotkey!("input_screenshot_btn"),
    },
    Setting {
        label: "override-remap-a",
        description: "override-desc-remap",
        keys: &["input_player1_btn_a"],
        target: Target::Remap,
        choices: remap!("input_player1_btn_a"),
    },
    Setting {
        label: "override-remap-b",
        description: "override-desc-remap",
        keys: &["input_player1_btn_b"],
        target: Target::Remap,
        choices: remap!("input_player1_btn_b"),
    },
    Setting {
        label: "override-remap-x",
        description: "override-desc-remap",
        keys: &["input_player1_btn_x"],
        target: Target::Remap,
        choices: remap!("input_player1_btn_x"),
    },
    Setting {
        label: "override-remap-y",
        description: "override-desc-remap",
        keys: &["input_player1_btn_y"],
        target: Target::Remap,
        choices: remap!("input_player1_btn_y"),
    },
    Setting {
        label: "override-remap-l",
        description: "override-desc-remap",
        keys: &["input_player1_btn_l"],
        target: Target::Remap,
        choices: remap!("input_player1_btn_l"),
    },
    Setting {
        label: "override-remap-r",
        description: "override-desc-remap",
        keys: &["input_player1_btn_r"],
        target: Target::Remap,
        choices: remap!("input_player1_btn_r"),
    },
    Setting {
        label: "override-remap-l2",
        description: "override-desc-remap",
        keys: &["input_player1_btn_l2"],
        target: Target::Remap,
        choices: remap!("input_player1_btn_l2"),
    },
    Setting {
        label: "override-remap-r2",
        description: "override-desc-remap",
        keys: &["input_player1_btn_r2"],
        target: Target::Remap,
        choices: remap!("input_player1_btn_r2"),
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::retroarch_config::parse;

    #[test]
    fn every_setting_starts_with_default_and_owns_what_it_writes() {
        for setting in OPTIONS.iter().chain(CONTROLS.iter()) {
            assert!(
                setting.choices[0].sets.is_empty(),
                "{} must offer Default first",
                setting.label
            );

            // A choice writing a key the setting does not own could never be cleared again
            for choice in setting.choices {
                for (key, _) in choice.sets {
                    assert!(
                        setting.keys.contains(key),
                        "{} writes {key} but does not own it",
                        setting.label
                    );
                }
            }
        }
    }

    #[test]
    fn unset_keys_read_as_default() {
        let empty = parse("");
        for setting in OPTIONS.iter().chain(CONTROLS.iter()) {
            assert_eq!(setting.current(&empty), 0, "{}", setting.label);
        }
    }

    #[test]
    fn a_written_choice_reads_back_as_itself() {
        for setting in OPTIONS.iter().chain(CONTROLS.iter()) {
            for (index, _) in setting.choices.iter().enumerate().skip(1) {
                let mut map = BTreeMap::new();
                for (key, value) in setting.changes(index) {
                    if let Some(value) = value {
                        map.insert(key.to_owned(), value);
                    }
                }
                assert_eq!(
                    setting.current(&map),
                    index,
                    "{} choice {index}",
                    setting.label
                );
            }
        }
    }

    #[test]
    fn default_clears_every_key_the_setting_owns() {
        for setting in OPTIONS.iter().chain(CONTROLS.iter()) {
            let changes = setting.changes(0);
            assert_eq!(changes.len(), setting.keys.len(), "{}", setting.label);
            assert!(
                changes.iter().all(|(_, value)| value.is_none()),
                "{} Default must remove, not write",
                setting.label
            );
        }
    }

    #[test]
    fn switching_choices_leaves_nothing_behind() {
        // Aspect ratio is the interesting one: two choices, one key, opposite values
        let aspect = &OPTIONS[0];
        let mut map = BTreeMap::new();
        for (key, value) in aspect.changes(1) {
            if let Some(value) = value {
                map.insert(key.to_owned(), value);
            }
        }
        // Now pick Default and confirm the key is dropped rather than left at its old value
        for (key, value) in aspect.changes(0) {
            assert!(value.is_none());
            map.remove(key);
        }
        assert!(map.is_empty());
    }

    #[test]
    fn hotkeys_and_remaps_go_to_different_files() {
        assert!(CONTROLS.iter().any(|s| s.target == Target::Cfg));
        assert!(CONTROLS.iter().any(|s| s.target == Target::Remap));
        assert!(OPTIONS.iter().all(|s| s.target == Target::Cfg));
    }

    /// Checks that every row with the given target agrees on what number a button label means.
    ///
    /// Split by target on purpose. Hotkeys are written as SDL joystick indices and remaps as
    /// RetroPad ids -- two namespaces that happen to carry identical numbers on this hardware,
    /// because RetroArch's default map assigns them one to one. Checking them together would tie
    /// the two together and fail for the wrong reason if the pad map ever changed.
    fn assert_buttons_agree(target: Target) {
        let mut seen: BTreeMap<&str, &str> = BTreeMap::new();

        for setting in CONTROLS.iter().filter(|s| s.target == target) {
            for choice in setting.choices {
                if !choice.label.starts_with("override-button-") {
                    continue;
                }
                let (_, value) = choice.sets[0];
                match seen.get(choice.label) {
                    Some(first) => assert_eq!(
                        *first, value,
                        "{} disagrees on {}",
                        setting.label, choice.label
                    ),
                    None => {
                        seen.insert(choice.label, value);
                    }
                }
            }
        }

        // So the test cannot pass by having found nothing to check
        assert!(seen.len() >= 10, "only found {} buttons", seen.len());
    }

    #[test]
    fn hotkey_rows_agree_on_their_button_numbers() {
        assert_buttons_agree(Target::Cfg);
    }

    #[test]
    fn remap_rows_agree_on_their_button_numbers() {
        assert_buttons_agree(Target::Remap);
    }
}
