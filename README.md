# Allium Flip

A personal fork of [Allium](https://github.com/goweiwen/Allium), by Wei Wen Goh and contributors,
for the **Miyoo Mini Flip**. Allium is a custom launcher for the Miyoo Mini family, in the spirit of
[OnionOS](https://github.com/OnionUI/Onion) and [MiniUI](https://github.com/shauninman/MiniUI).

This fork is trimmed to what one Flip needs and adds a handful of things on top. The upstream commit
history is not preserved; see the original repository for it. Allium is MIT licensed — see
[LICENSE](LICENSE).

## How this differs from upstream

- **Flip only.** The core set is trimmed to the consoles actually used; the stock/Onion card layout
  is still honoured.
- **Per-game CPU speed.** Six governor presets, set from the in-game menu's **Speed** row and applied
  instantly; the global default is under Settings → Power.
- **Display profiles**, replacing night mode: named warmth and dimness presets, cycled with
  Menu + Select, applied in the display controller so they cover games too.
- **Power**: what happens when a charger wakes a powered-off device (charge screen, charge silently,
  or stay off), and how long suspend lasts before powering off.
- **Updates over Wi-Fi**, no SD card: from the handheld's own Settings → System Update, or pushed
  from a computer — see [UPDATING.md](UPDATING.md).
- Hardening you will not see: settings files survive a bad write, the daemon survives a crashing
  launcher, Wi-Fi comes up with one supplicant, and LAN services start without internet access.

See [CHANGELOG.md](CHANGELOG.md) for what each release changed.

## Installation

### First install
1. Format the SD card to [FAT32](https://github.com/anzz1/DotUI-X/wiki/fat32format).
2. Download the latest [release](https://github.com/AidenPixxel/allium-flip/releases/latest) and
   extract it onto the card, e.g. `E:/`.
3. Eject the card properly.

### Updating
You never need to format again. See [UPDATING.md](UPDATING.md): from the handheld's System Update
screen, pushed over Wi-Fi from a computer, or by card.

## Features

- Stock / Onion / DotUI card layout, no configuration needed
- Box art (250 px wide, PNG, JPG, GIF); `gameslist.xml` with nested folders
- Favorites; recents by last played or playtime, with save-state previews; search
- Activity tracker
- RetroArch for every core, with per-game core selection
- Volume and brightness on Menu + L/R/U/D, with an on-screen indicator
- Display profiles (Menu + Select) and per-game CPU speed
- In-game menu: save and load with screenshots, Speed, Emulator (RetroArch's own menu),
  [guides](https://github.com/goweiwen/Allium/wiki/In-game-Guide-Walkthrough-Reader), disk changer,
  reset, quit
- Resume where you left off after a power cycle; suspend; configurable charger-wake behaviour
- Settings: System Update; Wi-Fi with file server, FTP, SSH, telnet, syncthing and NTP; clock and
  timezone; power; display; theme; language

## Development

### Requirements
`make`, nightly `cargo`, `zip`, `clang` (`libclang-dev`), `patchelf`, and
[cargo-zigbuild](https://github.com/rust-cross/cargo-zigbuild) with `zig`.

### Architecture
- `alliumd` — the daemon. Owns input, launches the launcher, game and menu, handles the volume and
  brightness hotkeys and their on-screen indicator, power and suspend. If it exits the boot script
  reboots the device, so it is written not to.
- `allium-launcher` — the main menu: games, recents, settings.
- `allium-menu` — the in-game menu, run as a thread inside `alliumd`.
- `activity-tracker`, `screenshot-viewer` — apps under `Apps/`.
- `screenshot`, `say`, `show` — small tools used by the boot script and the hotkeys.
- `myctl` — hardware control (volume, display) through the Miyoo proprietary libraries, via `ffi`.
- `common` — everything shared.

### Simulator
```
make simulator bin=allium-launcher
make simulator bin=allium-menu
```
There is none for `alliumd`. CI builds the simulator to catch breakage but does not run it.

### Building and CI
`make all` builds Allium and RetroArch and assembles `dist/`. CI runs `cargo fmt --check`, the tests,
clippy in both the host and `miyoo` configurations, and `cargo deny`. A push to `main` that builds
publishes a release tagged `v<version>-flip.<run>`; the format check, tests and clippy gate the
build.

### Deploying to hardware
Over Wi-Fi: `scripts/push-update.sh <ip>`, or `make push DEVICE=<ip>` — see
[UPDATING.md](UPDATING.md). By card: `make deploy SDCARD_PATH=/path/to/card`, or put `SDCARD_PATH`
in a git-ignored `local.mk`.

### Debugging on the device
- `allium.log` at the card root is `alliumd`'s log at `RUST_LOG=info`, readable over the file server
  at `http://<ip>/allium.log`.
- A file called `.debug` at the card root makes the boot script bring up Wi-Fi and telnet and **not**
  reboot when `alliumd` exits — the way out of a reboot loop.
- Apps → Terminal is a shell on the device.

## Troubleshooting

**Changes made in RetroArch's own menu don't survive closing the game.** RetroArch writes its
`retroarch.cfg` back only when `config_save_on_exit` is on; cards set up with Onion-derived defaults
have it off. In RetroArch: Settings → Configuration → **Save Configuration on Quit** → On, then Main
Menu → Configuration File → **Save Current Configuration** once. Until this fork's `v1.0.1-flip.39`,
games started with **Restart** also discarded every save regardless of that setting.

## Acknowledgements

Allium is only possible thanks to the Miyoo Mini community, including but not limited to:
- eggs: RetroArch port, [many code samples](https://www.dropbox.com/sh/hqcsr1h1d7f8nr3/AABtSOygIX_e4mio3rkLetWTa), answering questions on Discord
- [Onion team](https://github.com/OnionUI/Onion) (Aemiii91, Schmurtz, Totofaki, and more): maintaining a sane-defaults RetroArch configuration, and the huge village
- kebabstorm: [Miyoo Mini resources](https://github.com/anzz1/miyoomini-resources)
- shauninman: Allium is heavily inspired by [MiniUI](https://github.com/shauninman/MiniUI)'s simplicity and clean design
- [steward-fu](https://github.com/steward-fu): miraculous DraStic port
- Early adopters and testers of Allium
- [Icons8.com](https://icons8.com) for the icons used in the upstream wiki

## Community

Upstream Allium has a Discord at https://discord.gg/eSvVqe6c8n. This fork is a personal one and is
not supported there.
