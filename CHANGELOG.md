# Changelog

Every push to `main` that builds is a release, tagged `v<version>-flip.<run>`. This file groups those
by what changed rather than by run number; the commit log has the detail.

## Unreleased — from MinUI

After a survey of [MinUI](https://github.com/shauninman/MinUI) and
[MyMinUI](https://github.com/Turro75/MyMinUI). Most of what they do was already here — boot into
the last game, lid handling, the headphone mute, native 752×560 — or belongs to RetroArch. What
transferred is about power.

- **Suspend saves the game first.** Suspend was SIGSTOP and a dark panel, and no power path ever
  asked RetroArch to save; a flat battery or a fall while asleep lost everything since RetroArch's
  last own auto-save. Now the daemon asks for one first, waits for the file to finish landing and
  syncs the card before stopping anything. The state is the `.state.auto` that resume reads.
- **A suspended device draws less.** The panel's power line is pulled low, not just the PWM
  stopped, and the CPU drops to the cpufreq floor instead of sitting pinned at 1.2 GHz behind a dark
  panel. Both are put back on wake.
- **An overclock, as one Power row.** Stock 1.2 GHz, 1.3 GHz or 1.5 GHz for games — MinUI's Normal
  and Performance tiers, reached the way MinUI reaches them: by reprogramming the SSD202D's MPLL,
  which is how it gets past the kernel's own table. Default Stock, applied at game launch, undone
  at exit, re-applied after a wake. Not the removed performance modes, which only ever capped the
  clock *below* stock.
- **A fresh install sleeps when the lid shuts.** The default was Shutdown. Existing `power.json`
  files keep their value.

## Unreleased — rebuilt on upstream

This fork was rebuilt from [upstream](https://github.com/goweiwen/Allium) `main` rather than carried
forward from its own history. Every feature below was chosen deliberately and re-applied; anything
not listed is upstream's own behaviour, unchanged.

What that gains: three upstream commits the fork had never taken — row-wise framebuffer reads and
writes, the OTA client's TLS fix, and version 1.0.2 — and the shedding of two features that had been
built and then removed again inside the fork, along with the residue they left behind.

### Kept from the old fork

- **Display profiles.** Three named presets, each with a full set of panel values plus its own warmth
  and backlight brightness, rotated with Menu + Select. Applied in the display controller, so they
  cover games. Night dims the lamp; Day restores it.
- **A flicker-free in-game indicator**, handed to RetroArch as its own on-screen message while a game
  is running. Needs the bundled RetroArch patch that makes `SHOW_MSG` replace the previous message
  rather than queue behind it. Everywhere else keeps the drawn plate.
- **Suspend shutdown delay** and **charger-wake behaviour** (charge screen, charge silently, or stay
  off — decided before the backlight comes on), with a description on every Power row.
- **Wi-Fi that reports why it failed**, and that comes up with one supplicant on a network with no
  route to the internet.
- **Updates over Wi-Fi**: on-device System Update against this fork's releases, builds pushed from a
  computer, and an install that is staged, atomic, resumable, and preserves hand-edited
  `consoles.toml` / `cores.toml`.
- **X restarts a game from the beginning**; A resumes, Y sorts, Select opens options.
- Hardening: settings files kept as `.bak` rather than deleted when they fail to parse and written
  atomically; the daemon logging and retrying instead of exiting, since the boot script answers a
  dead daemon by rebooting; stale autorepeats dropped without dropping real presses; the in-game menu
  no longer acting on a play session's queued input; `myctl display blank` no longer aborting; ffplay
  putting the CPU governor back when it is done with it.
- CI: clippy over both feature sets and the simulator build gate the device build, and a release is
  published from the artefact that was tested rather than a rebuild of it.

### Deliberately not carried over

- **Per-game CPU speed / performance modes.** Built and then removed inside the old fork: capping the
  clock on a frame-limited emulator saves no measurable battery without per-frequency voltage
  scaling, and its "High" preset was untuned `ondemand`, slower than leaving the CPU alone on a
  kernel that boots with `performance`.
- **The in-game Controls and Options screens.** Also built and then removed: they wrote RetroArch
  override files, which collide with RetroArch's own settings and cannot apply without a relaunch.
- **The brightness curve.** The slider keeps the platform's plain mapping rather than a logarithmic
  one over the panel's full PWM range, so every step is a distinct duty cycle.
- **Discoverable per-game core selection** and the **Restart config carry-back** — see the caveat
  below.

### Removed from upstream

- **Game search.** The results screen, the keyboard-backed search view, the commands behind them, the
  SQL query, and the sort-by-relevance option are gone, which is what frees X for Restart. The guide
  reader keeps its own in-text search.
- **61 emulator cores and the PICO-8 wrapper**, about 300 MB. `consoles.toml` and `cores.toml` are
  untouched, so restoring a core is a matter of dropping its `.so` and `.info` back in — and 52 of
  the 85 listed consoles have no shipped core as a result.
- The DraStic download from the build, the scheduled nightly workflow, and the separate release
  workflow that `ci.yml` now supersedes.

### Known caveat

A game launched with **X** discards anything saved in RetroArch's own menu during that session,
hotkeys included, because Restart runs RetroArch against a temporary config and does not copy it
back. The fix for this existed in the old fork and was not selected. Launch with **A** when those
changes need to persist.
