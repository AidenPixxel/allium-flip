# Allium Flip

A personal fork of [Allium](https://github.com/goweiwen/Allium), by Wei Wen Goh and contributors,
for the **Miyoo Mini Flip**. Allium is a custom launcher for the Miyoo Mini family, in the spirit of
[OnionOS](https://github.com/OnionUI/Onion) and [MiniUI](https://github.com/shauninman/MiniUI).

This fork is rebuilt on upstream rather than diverged from it: it is upstream `main`, plus a trim to
one device's core set, plus a chosen handful of features. Allium is MIT licensed — see
[LICENSE](LICENSE).

## How this differs from upstream

- **Flip only.** 61 of upstream's 95 emulator cores and the PICO-8 wrapper are not shipped, which is
  about 300 MB of card space. `consoles.toml` and `cores.toml` are deliberately left intact, so
  dropping a core's `.so` and `.info` back into `RetroArch/.retroarch/cores` restores that system
  with no configuration change — and 52 of the 85 listed consoles have no shipped core as a result.
- **Display profiles.** Three named presets — Day, Night, Custom — each holding a full set of panel
  values plus its own warmth and backlight brightness. Menu + Select rotates between them and shows
  the name; they are applied in the display controller, so they cover games too.
- **A flicker-free in-game indicator.** Volume, brightness and profile changes are handed to
  RetroArch as its own on-screen message while a game is running, because a plate stamped into
  RetroArch's framebuffer cannot be timed not to flicker. Everywhere else keeps the drawn plate.
- **Power**: how long suspend lasts before powering off, and what happens when a charger wakes a
  powered-off device — charge screen, charge silently, or stay off, decided before the backlight
  comes on. Every Power row explains the option you are looking at.
- **Wi-Fi that says why it failed**: Searching, Connecting, Getting IP address — then the likely
  cause, rather than "Connecting..." forever.
- **Updates over Wi-Fi**, no SD card: from the handheld's own Settings → System Update, or pushed
  from a computer — see [UPDATING.md](UPDATING.md).
- **Launcher buttons**: A resumes, **X restarts from the beginning**, Y sorts, Select opens options.
- **No game search.** Upstream searches games by name from a keyboard view; this fork does not, which
  is what frees X. The guide reader keeps its own in-text search.
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
- Favorites; recents by last played or playtime, with save-state previews
- Activity tracker
- RetroArch for every shipped core, with per-game core selection
- Volume and brightness on Menu + L/R/U/D, with an on-screen indicator
- Display profiles on Menu + Select
- In-game menu: save and load with screenshots, Emulator (RetroArch's own menu),
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
There is none for `alliumd`. CI builds the simulator because it shares almost every view with the
device and nothing else compiles it — a launcher change that breaks it is a real break, and this is
the job that catches it.

### Building and CI
`make all` builds Allium and RetroArch and assembles `dist/`. CI runs `cargo fmt --check`, the
tests, clippy in both the host and `miyoo` configurations, the simulator, and `cargo deny`; the
device build waits on all of them. A push to `main` that builds publishes a release tagged
`v<version>-flip.<run>`.

### RetroArch patches
`patches/retroarch/` holds Allium's own patches to RetroArch's source. The Makefile copies them into
RetroArch-patch's own `patches/` before the Docker build, since its Makefile applies every
`patches/*.patch` in sorted order. That loop reports only the last patch's status, so
`scripts/retroarch/verify-patches.sh` runs afterwards inside the container and fails the build if any
Allium patch is missing from the built tree. A build directory that was already patched will not pick
up a new one; run `make clean` in `third-party/RetroArch-patch` first.

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

**A game started with X loses settings I saved in RetroArch's own menu.** Known, and inherent to how
Restart works here: it runs RetroArch against a temporary config with save-state auto-load turned
off, and does not copy that config back when the game exits. Anything changed in RetroArch's menu
during a Restart-launched session — hotkeys included — is discarded. Launch with **A** when those
changes need to persist.

**Other changes made in RetroArch's own menu don't survive closing the game.** RetroArch writes its
`retroarch.cfg` back only when `config_save_on_exit` is on; cards set up with Onion-derived defaults
have it off. In RetroArch: Settings → Configuration → **Save Configuration on Quit** → On, then Main
Menu → Configuration File → **Save Current Configuration** once.

## Acknowledgements

Allium is only possible thanks to the Miyoo Mini community, including but not limited to:
- eggs: RetroArch port, [many code samples](https://www.dropbox.com/sh/hqcsr1h1d7f8nr3/AABtSOygIX_e4mio3rkLetWTa), answering questions on Discord
- [Onion team](https://github.com/OnionUI/Onion) (Aemiii91, Schmurtz, Totofaki, and more): maintaining a sane-defaults RetroArch configuration, and the huge village
- kebabstorm: [Miyoo Mini resources](https://github.com/anzz1/miyoomini-resources)
- shauninman: Allium is heavily inspired by [MiniUI](https://github.com/shauninman/MiniUI) for its simplicity and clean design
- [steward-fu](https://github.com/steward-fu): miraculous DraStic port
- Early adopters and testers of Allium
- [Icons8.com](https://icons8.com) for the icons used in the upstream wiki

## Community

Upstream Allium has a Discord at https://discord.gg/eSvVqe6c8n. This fork is a personal one and is
not supported there.
