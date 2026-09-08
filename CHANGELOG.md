# Changelog

Every push to `main` that builds is a release, tagged `v<version>-flip.<run>`. This file groups those
by what changed rather than by run number; the commit log has the detail.

## Unreleased

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
- CI runs clippy in both the host and `miyoo` configurations and builds the simulator, advisory
  until their first clean run.
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
