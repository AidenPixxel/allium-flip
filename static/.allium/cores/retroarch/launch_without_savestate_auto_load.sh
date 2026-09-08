#!/bin/sh
# Launches RetroArch with save-state auto-load turned off for this one run -- what Restart means --
# without losing anything RetroArch saves on the way out.
#
# RetroArch reads savestate_auto_load only from a config file, so the setting is flipped in a copy
# on tmpfs and RetroArch is pointed at the copy with -c. But -c also decides where "save on exit"
# and Configuration File > Save Current Configuration write to, so everything changed in
# RetroArch's menu during such a run used to land in /tmp and vanish: a hotkey set from a
# Restart-launched game never persisted. The copy is now carried back over the real config once
# RetroArch exits, with auto-load restored to what it was.
DIR="${RETROARCH_DIR:-/mnt/SDCARD/RetroArch}"
CFG="$DIR/.retroarch/retroarch.cfg"
TMP="${RETROARCH_TMP_CFG:-/tmp/retroarch.cfg}"
# The three variables above exist so the copy-back logic can be exercised off the device
PRELOAD="${RETROARCH_PRELOAD-libpadsp.so}"

if [ ! -f "$CFG" ]; then
	# No config to flip means nothing to preserve either
	HOME="$DIR" LD_PRELOAD="$PRELOAD" exec "$DIR/retroarch" -v -L "$DIR/.retroarch/cores/$1_libretro.so" "$2"
fi

cp "$CFG" "$TMP"
autoload_was_on=0
if grep -q '^savestate_auto_load = "true"' "$TMP"; then
	autoload_was_on=1
	sed -i 's/^savestate_auto_load = "true"/savestate_auto_load = "false"/' "$TMP"
fi

# Not exec: this shell has to outlive RetroArch to copy the config back. alliumd stops a game by
# sending this process SIGTERM, so pass it on. A trapped signal also interrupts `wait`, while
# RetroArch is still shutting down and writing its config -- hence waiting again until it is gone.
trap 'kill -TERM "$pid" 2> /dev/null' TERM INT
HOME="$DIR" LD_PRELOAD="$PRELOAD" "$DIR/retroarch" -v -L "$DIR/.retroarch/cores/$1_libretro.so" "$2" -c "$TMP" &
pid=$!
wait "$pid"
status=$?
while kill -0 "$pid" 2> /dev/null; do
	wait "$pid"
	status=$?
done
trap - TERM INT

# Carry saved settings back, minus the flag forced above. If RetroArch never rewrote the copy it
# still equals the original and nothing happens. A copy much smaller than the original is one a
# crash cut short, and must not replace the real file.
if [ -s "$TMP" ] && [ "$(wc -c < "$TMP")" -ge $(($(wc -c < "$CFG") / 2)) ]; then
	if [ "$autoload_was_on" = 1 ]; then
		sed -i 's/^savestate_auto_load = "false"/savestate_auto_load = "true"/' "$TMP"
	fi
	if ! cmp -s "$TMP" "$CFG"; then
		cp "$TMP" "$CFG.new" && mv -f "$CFG.new" "$CFG"
	fi
fi

exit "$status"
