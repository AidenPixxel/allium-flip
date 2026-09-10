# Changelog

Every push to `main` that builds is a release, tagged `v<version>-flip.<run>`. This file groups those
by what changed rather than by run number; the commit log has the detail.

## Unreleased

### Fixed
- The in-game volume and brightness indicator no longer flickers over RetroArch. It is now handed to
  RetroArch as a `SHOW_MSG`, which RetroArch draws inside the frame it presents; stamping the same
  pixels into the framebuffer from outside always raced the game's next frame, which flips without
  vsync. Needs the bundled RetroArch patch that makes `SHOW_MSG` replace the previous message rather
  than queue behind it.

### Changed
- **Night mode now dims the backlight.** A display profile carries its own **Brightness**, applied
  when the profile becomes active — so Menu+Select into Night drops the lamp and back into Day
  restores it. This replaces **Dimness**, which scaled the video signal while the backlight burned
  just as hard: it lowered peak white but not the black level, costing contrast and saving no light.
  Menu+Up/Down still nudges the live backlight; the next profile switch takes it back to that
  profile's value.
- **Warmth reaches a real amber.** At 100 it now leaves roughly 1800–2000K instead of 2500K. The old
  per-channel floor that capped it was guarding against the panel's grey-flattening, which needs
  *all three* channels below 15 and so could only ever happen to a profile whose own red was that
  low; that case now lifts the whole set proportionally and keeps the tint.
- **The brightness slider is spaced by ratio, not by difference.** Each step is the same multiple of
  the last, and the dimmest setting is three times dimmer than before. Previously one step near the
  bottom nearly tripled the light while one at the top changed it by a twentieth.
- The shipped **Night** profile is set up for a dark room: brightness 20, warmth 100, saturation 35,
  contrast 40, luminance untouched.

**On upgrade:** a given brightness *number* now means less light — nudge it up once and it sticks.
Warmth 100 is noticeably warmer than it was. Existing profiles that had Dimness above zero become
dim-backlight profiles; the rest keep the brightness they had.

- The indicator alliumd draws itself — in the launcher, the in-game menu and Allium's own apps — now
  looks exactly like RetroArch's in-game message: one line of white 2× bitmap text with a black drop
  shadow at the bottom left, no plate or icons, shown for three seconds. Same key press, same look,
  wherever you are. The font is Spleen 6×12 (BSD 2-Clause; licence shipped in
  `.allium/fonts/licenses/`), converted into the binary by `scripts/fonts/bdf_to_rust.py`.
- Games started with **Restart** kept RetroArch's config on tmpfs, so every setting saved from
  RetroArch's menu during such a run -- hotkeys included -- was discarded on exit. The launcher now
  carries the saved config back with auto-load restored.

## v1.0.1-flip.38 -- 2026-09-08

### Fixed
- Settings files (`power.json`, `wifi.json`, `locale.json`, `current_game`, `update.json`) are kept
  as `.bak` instead of deleted when they fail to parse, and written atomically so a power cut cannot
  leave a truncated one behind.
- The daemon no longer reboots the device when the launcher crashes on startup (three exits in ten
  seconds now pause five), when it cannot spawn the launcher (it retries), when the battery reading
  fails at boot, or when a key event's handler errors.
- Wi-Fi: `wifi-on.sh` leaves exactly one `wpa_supplicant` and one `udhcpc` however often it runs,
  and only power-cycles the radio when the interface will not come up without it. Open networks
  (`key_mgmt=NONE`) and hidden networks (`scan_ssid=1`) are configured correctly; an empty password
  no longer invalidates the whole `wpa_supplicant.conf`.
- LAN services (file server, FTP, SSH, telnet, syncthing) wait for `wlan0` to have an address rather
  than for `1.1.1.1` to answer, so they start on networks with no route to the internet.
- `dufs-on.sh` no longer uses `&>`, which busybox `sh` misparses.
- Repopulate Database keeps rows that carry a per-game performance mode.
- The in-game OSD probes whether `FBIO_WAITFORVSYNC` really blocks before trusting it, and sweeps
  blits up to the moment the panel scans the plate rather than blitting once at vblank.
- On-device update: the install is staged and each file moved into place atomically; the archive is
  removed only once everything has moved, so an interrupted install resumes on the next boot.
  Hand-edited `consoles.toml` and `cores.toml` are preserved via a `.dist` copy of what shipped.

### Changed
- The in-game **Controls** and **Options** screens are removed. They wrote RetroArch override files,
  which collided with RetroArch's own settings, could not apply without a relaunch, and could not be
  presented legibly in a menu with no frame timer. Per-game Performance returns as its own **Speed**
  row, applied instantly.
- The Wi-Fi status row reports progress (Searching / Connecting / Getting IP address) and, after
  thirty seconds, the likely cause: network not found, wrong password, or no IP address.
- Update channel is On/Off (was Off/Stable/Nightly) and defaults to On. "Update available" means
  numerically newer, not merely different.

### Added
- **System Update** on the device works: TLS via `rustls` with bundled roots (upstream `392f7f2`),
  pointed at this fork's releases.
- CI runs clippy in both the host and `miyoo` configurations and gates the build on it; it also
  builds the simulator.
- `.gitattributes` pins LF for every file the handheld reads.
- This changelog, and a README describing the fork rather than upstream.

## 2026-09-08

- Push a build to the handheld over Wi-Fi with `scripts/push-update.sh` / `make push`; the archive
  is verified before upload and again on the device before extraction; a bad archive is quarantined
  as `.bad` instead of retried every boot. UPDATING.md documents the routes.
- Power-screen descriptions scroll instead of running off screen.
- Controls and Options in-game screens with per-game / per-console / per-core scoping, and Apply
  Now (all removed again — see Unreleased).

## 2026-09-07

- Per-game CPU governor presets, later reworked to six frequency-capped tiers (Powersave, Low,
  Medium, High, Max, System) with a global default under Settings → Power.
- Named display profiles, replacing night mode.
- Suspend shutdown delay is a setting.
- Charger-wake keeps the panel dark.
- `myctl` no longer aborts on a mismatched clap argument id.
- OSD driven from the panel's vblank rather than the app's page flips.

## 2026-09-06 — fork created

- Baseline trimmed from upstream Allium at about `b254538`, Flip-only core set.
- Night mode; charger boot behaviour (charge screen, charge silently, stay off); discoverable
  per-game core selection; in-game menu drops stale input before reading; OSD bar flicker fix.
- CI: releases published from the build that already succeeded; `Off` update channel as default
  (the payload pointed at upstream at the time).
