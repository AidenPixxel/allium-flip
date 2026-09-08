# Updating

How to get a new build onto the Miyoo Mini Flip. The Wi-Fi route needs no SD card and is the one
to use day to day; the card route is the fallback.

**You never need to format.** Formatting is for a first install only. Every update overwrites a
handful of folders and leaves your games and saves alone.

---

## Over Wi-Fi

### Once, on the handheld

**Settings → Wi-Fi**, then set:

| Setting | Value |
| --- | --- |
| Wi-Fi Enabled | On |
| Web File Explorer | On |

Note the **IP Address** shown on the same screen — for example `192.168.1.42`. It is worth giving
the handheld a static lease on your router so this does not change.

### Every update

On Linux or macOS:

```bash
make push DEVICE=192.168.1.42
```

Put `DEVICE=192.168.1.42` in a `local.mk` at the repo root and it becomes just `make push` — the
Makefile already includes that file, the same way it picks up `SDCARD_PATH`.

Without `make`, the script does the same thing:

```bash
./scripts/push-update.sh 192.168.1.42
```

On Windows:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\push-update.ps1 -Device 192.168.1.42
```

Then **restart the handheld.** It shows "Updating Allium. Please wait...", installs, and reboots
itself. Check **Settings → System Update → Allium Version** afterwards to confirm the new version
took.

### Pushing a build that was never released

Any zip works — a CI artifact from a side branch, say. Pass it as the second argument:

```bash
./scripts/push-update.sh 192.168.1.42 ~/Downloads/allium-armv7-unknown-linux-gnueabihf.zip
```

```powershell
.\scripts\push-update.ps1 -Device 192.168.1.42 -Zip $HOME\Downloads\allium-armv7-unknown-linux-gnueabihf.zip
```

### Doing it by hand

The scripts only add fetching and checking around a single upload. If you already have the zip:

```bash
curl -f -T allium-armv7-unknown-linux-gnueabihf.zip http://192.168.1.42/allium-ota.zip
```

On Windows use `curl.exe`, not `curl` — the latter is a PowerShell alias for `Invoke-WebRequest`
and does not accept `-T`.

The filename matters. The boot script looks for exactly `/mnt/SDCARD/allium-ota.zip`, which is
what the Web File Explorer's root maps to.

---

## By SD card

Take the card out and, from the repo:

```bash
make deploy SDCARD_PATH=/media/you/SDCARD
```

That copies only `.allium`, `.tmp_update`, `Apps`, `RetroArch` and `Themes`, and only files that
are newer. Use `make deploy-all` to also delete files no longer in the build.

By hand, extract these folders from the release zip over the card:

- `.allium`
- `.tmp_update`
- `Apps`
- `RetroArch`

Then eject the card properly — **this matters**, or you can be left with a half-written
`.tmp_update` and nothing to boot.

---

## What an update touches

| Kept | Replaced |
| --- | --- |
| `Roms/` | `.allium/bin/`, `scripts/`, `locales/`, `cores/` |
| `Saves/` — including save states, `allium.db`, and RetroArch's own per-game config | `.allium/config/consoles.toml` and `cores.toml` |
| `BIOS/` | `.tmp_update/` |
| `.allium/state/` — Wi-Fi, display, power, theme and language settings | `RetroArch/` and its core set |
| `Screenshots/` | `Apps/`, `Themes/` |

The one to watch is **`.allium/config/consoles.toml` and `cores.toml`**. Hand edits to either are
lost on every update. Keep a copy if you have customised them.

---

## Troubleshooting

**The script will not run on Windows.** Script execution is off by default. Either use the
`-ExecutionPolicy Bypass` form above, or allow local scripts once:

```powershell
Set-ExecutionPolicy -Scope CurrentUser RemoteSigned
```

**Upload fails, or the browser cannot reach the handheld.** Check the IP has not changed, and that
**Web File Explorer** is still On. Also note the handheld pings `1.1.1.1` before starting that
server — on a LAN with no route to the internet it never comes up.

**"The archive is incomplete or corrupt."** The download was cut short. Run it again; nothing was
uploaded, and nothing on the handheld was touched.

**Nothing happens after restarting.** Confirm the file landed as `allium-ota.zip` at the card root
and not under a subfolder. The installer also needs 300 MB free, and gives up with a message on
screen if there is less.

**A file called `allium-ota.zip.bad` has appeared.** The installer found the archive incomplete and
refused it, deliberately moving it aside so it does not retry on every boot. Delete it and push
again.

**The version did not change.** The scripts take the *latest release*, so check CI has finished
publishing before pushing. `Settings → System Update` shows the installed version.

---

## Why "System Update" on the device does not do this

The screen exists and is reachable, but it cannot work in this build for two reasons:

1. `reqwest` is compiled with no TLS backend, so `https://api.github.com` is unreachable from the
   handheld — there is nothing to perform the handshake.
2. `GITHUB_REPOSITORY` in `crates/allium-launcher/src/ota.rs` still points at upstream
   `goweiwen/Allium`. Left as-is, an update would restore every core trimmed from this build and
   replace its binaries with stock ones — which is why the update channel defaults to **Off**.

Doing the download on a computer, where TLS already works, and uploading over plain HTTP on the LAN
avoids both. Making the on-device screen work needs a TLS stack added to the armv7 cross-build and
the repository re-pointed; until then, use the Wi-Fi push above.
