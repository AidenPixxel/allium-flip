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

### Every update — Linux and macOS

You need `curl` and `unzip`:

```bash
sudo apt install curl unzip      # Debian, Ubuntu
sudo dnf install curl unzip      # Fedora
sudo pacman -S curl unzip        # Arch
```

**If you have the repo cloned**, run this from its root:

```bash
./scripts/push-update.sh 192.168.1.42
```

`make push DEVICE=192.168.1.42` does the same thing. Once it works, put `DEVICE=192.168.1.42` in a
`local.mk` at the repo root and it shortens to just `make push` — the Makefile already includes
that file, the same way it picks up `SDCARD_PATH`.

**If you do not have the repo**, the script stands alone. Fetch just it:

```bash
curl -fLO https://raw.githubusercontent.com/AidenPixxel/allium-flip/main/scripts/push-update.sh
chmod +x push-update.sh
./push-update.sh 192.168.1.42
```

Either way you should see:

```
Fetching the latest release from AidenPixxel/allium-flip ...
######################################################## 100.0%
Checking the archive ...
Uploading to http://192.168.1.42/allium-ota.zip ...
######################################################## 100.0%

Uploaded. Restart the handheld to install it.
```

**Or with no script at all** — this is the whole mechanism, and it needs nothing but `curl`:

```bash
curl -fLO https://github.com/AidenPixxel/allium-flip/releases/latest/download/allium-armv7-unknown-linux-gnueabihf.zip
curl -f -T allium-armv7-unknown-linux-gnueabihf.zip http://192.168.1.42/allium-ota.zip
```

The only thing that matters is the destination name. The boot script looks for exactly
`/mnt/SDCARD/allium-ota.zip`, and the Web File Explorer's root is the card root — so
`http://<ip>/allium-ota.zip` is that file. What you skip by doing it this way is the check that the
download completed, which is what the script adds.

### Every update — Windows

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\push-update.ps1 -Device 192.168.1.42
```

Use `curl.exe` rather than `curl` in any manual command — `curl` in PowerShell is an alias for
`Invoke-WebRequest`, which does not accept `-T`.

### Then restart the handheld

It shows "Updating Allium. Please wait...", installs, and reboots itself. Check
**Settings → System Update → Allium Version** afterwards to confirm the new version took.

### Pushing a build that was never released

Any zip works — a CI artifact from a side branch, say. Pass it as a second argument:

```bash
./scripts/push-update.sh 192.168.1.42 ~/Downloads/allium-armv7-unknown-linux-gnueabihf.zip
```

```powershell
.\scripts\push-update.ps1 -Device 192.168.1.42 -Zip $HOME\Downloads\allium-armv7-unknown-linux-gnueabihf.zip
```

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
| `Roms/`, `BIOS/`, `Screenshots/` | `.allium/bin/`, `scripts/`, `locales/`, `cores/`, `fonts/` |
| `Saves/` — save states, `allium.db`, RetroArch's own per-game config | `.tmp_update/` |
| `.allium/state/` — Wi-Fi, display, power, theme and language settings | `RetroArch/` and its core set |
| `.allium/config/consoles.toml` and `cores.toml`, **if you have edited them** | `Apps/`, `Themes/` (your additions stay; shipped files are refreshed) |

**Hand-edited `consoles.toml` and `cores.toml` survive.** The installer keeps what each release
shipped as `consoles.toml.dist` beside the file. If yours still matches it, you get the new one; if
it does not, yours stays and the new one lands as `.dist` for you to merge by hand. The first update
that carries this logic has no `.dist` to compare against and installs the shipped file — so an edit
made before it is overwritten once.

**The install is staged.** The archive is unpacked beside the installation and each file is then
moved into place, so a power cut part-way leaves whole files rather than half-written ones — and the
archive stays put to finish the job on the next boot. It needs free space for the unpacked archive;
the screen says how much if there is not enough.

--- | --- |
| `Roms/` | `.allium/bin/`, `scripts/`, `locales/`, `cores/` |
| `Saves/` — including save states, `allium.db`, and RetroArch's own per-game config | `.allium/config/consoles.toml` and `cores.toml` |
| `BIOS/` | `.tmp_update/` |
| `.allium/state/` — Wi-Fi, display, power, theme and language settings | `RetroArch/` and its core set |
| `Screenshots/` | `Apps/`, `Themes/` |

The one to watch is **`.allium/config/consoles.toml` and `cores.toml`**. Hand edits to either are
lost on every update. Keep a copy if you have customised them.

---

## Troubleshooting

### Linux and macOS

**`bash: ./push-update.sh: Permission denied`** — the file is not executable:

```bash
chmod +x push-update.sh
```

**`./scripts/push-update.sh: No such file or directory`** — you are not in the repo root. Either
`cd` there, or use the standalone download above, which puts the script in the current directory.

**`make: *** No rule to make target 'push'`** — same cause: `make` must be run from the repo root.

**`unzip is required but not installed`** — install it; the script uses it to check the archive
before uploading. See the install lines at the top of this page.

**`curl: (7) Failed to connect to 192.168.1.42 port 80`** — nothing is listening. Check the IP on
the handheld has not changed, and that **Web File Explorer** is still On.

### Windows

**The script will not run.** Script execution is off by default. Either use the
`-ExecutionPolicy Bypass` form above, or allow local scripts once:

```powershell
Set-ExecutionPolicy -Scope CurrentUser RemoteSigned
```

**`curl : The term '-T' is not recognized`** — you used `curl`, which PowerShell aliases to
`Invoke-WebRequest`. Use `curl.exe`.

### Either platform

**The file server is not reachable at all.** The handheld pings `1.1.1.1` before starting it, so on
a LAN with no route to the internet it never comes up.

**"The archive is incomplete or corrupt."** The download was cut short. Run it again; nothing was
uploaded, and nothing on the handheld was touched.

**Nothing happens after restarting.** Confirm the file landed as `allium-ota.zip` at the card root
and not under a subfolder. The installer also needs room to unpack the archive beside the
installation, and says how much on screen if there is not enough.

**A file called `allium-ota.zip.bad` has appeared.** The installer found the archive incomplete and
refused it, deliberately moving it aside so it does not retry on every boot. Delete it and push
again.

**The version did not change.** The scripts take the *latest release*, so check CI has finished
publishing before pushing. `Settings → System Update` shows the installed version.

---

## System Update on the device

**Settings → System Update** checks this fork's GitHub releases over Wi-Fi and installs the latest
one — the same archive the push script uploads, downloaded by the handheld itself and verified
against the SHA-256 GitHub publishes for it. Use it when the handheld has internet access; use the
push route above when it does not, or to install a build that was never released.

"Update available" means a *newer* version, not merely a different one. Tags are compared
numerically, so a handheld running a side-branch build newer than the latest release is not offered
a downgrade.

**Update Channel** is On by default. Set it Off to stop the handheld contacting GitHub at all.
