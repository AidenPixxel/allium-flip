#!/bin/sh
#
# Installs /mnt/SDCARD/allium-ota.zip. Run by the boot script from a copy on tmpfs, since the
# install overwrites this very file.
#
# The install is staged. Extracting straight over the live installation, as this used to, meant a
# power cut part-way could leave a half-written alliumd or updater -- the one way to brick the
# device short of a bad card. Now the archive is unpacked beside the installation and each file is
# moved into place, and a rename on the same filesystem is atomic: after a power cut every file is
# either the old one or the new one, whole. The archive is removed only once everything has moved,
# so an interrupted install simply resumes on the next boot.

ROOT=/mnt/SDCARD
ZIP="$ROOT/allium-ota.zip"
STAGE="$ROOT/.allium-ota-stage"

# Check if update file exists
if [ ! -f "$ZIP" ]; then
	echo "Update file not found at $ZIP" >&2
	exit 1
fi

# Sets the archive aside so `updater`, which only checks that the file exists, does not announce
# an update and fail again on every boot from here on
quarantine() {
	rm -rf "$STAGE"
	mv -f "$ZIP" "$ZIP.bad"
	say "$1"
	sleep 3
	exit 1
}

show --clear

# A stage left by an interrupted run would count against the free-space check below
rm -rf "$STAGE"

# Prove the archive is whole before doing anything to the card. A download cut short is the common
# failure and it is cheap to catch: a zip keeps its index at the end of the file, so an incomplete
# one cannot even be listed. This costs a read rather than a write.
listing="$(miniunz -l "$ZIP" 2> /dev/null)" || quarantine "Update file was incomplete. Nothing was changed."

# Staging needs room for the unpacked archive as well as the installed one. The listing gives the
# uncompressed size of every entry; if it does not parse, fall back to a generous flat figure.
unpacked_mb="$(printf '%s\n' "$listing" | awk '$1 ~ /^[0-9]+$/ { s += $1 } END { printf "%d", s / 1048576 }')"
[ "${unpacked_mb:-0}" -gt 0 ] || unpacked_mb=500
need_mb=$((unpacked_mb + 50))
available_mb=$(df -m "$ROOT" | tail -1 | awk '{print $4}')
if [ "${available_mb:-0}" -lt "$need_mb" ]; then
	echo "Need ${need_mb}MB free to install the update, have ${available_mb}MB." >&2
	say "Not enough space to update: need ${need_mb}MB, have ${available_mb}MB."
	sleep 5
	exit 1
fi

say "Updating Allium. Please wait..."

# Unpack beside the installation
mkdir -p "$STAGE"
if ! miniunz -x -o "$ZIP" -d "$STAGE/" > /dev/null; then
	echo "Update extraction failed." >&2
	# An archive that verified but still failed to extract means the card is full or failing,
	# and retrying it every boot would only loop
	quarantine "Update could not be unpacked. Nothing was changed."
fi

# Keep a hand-edited console or core map. Each release's shipped copy is recorded beside the file
# as .dist; a file that still matches its .dist is unmodified and takes the new one, and a file
# that differs is the user's and stays, with the new one landing as .dist to merge by hand. With
# no .dist to compare against -- the first update carrying this logic -- the shipped file wins.
preserve_config() {
	shipped="$STAGE/.allium/config/$1"
	installed="$ROOT/.allium/config/$1"
	dist="$installed.dist"
	[ -f "$shipped" ] || return 0
	if [ -f "$installed" ] && [ -f "$dist" ] && ! cmp -s "$installed" "$dist"; then
		echo "$1 was edited by hand; keeping it, new version saved as $1.dist"
		cp -f "$shipped" "$shipped.dist"
		cp -f "$installed" "$shipped"
	else
		cp -f "$shipped" "$shipped.dist"
	fi
}
preserve_config consoles.toml
preserve_config cores.toml

# Move every file into place. `find` output is read line by line, so paths with spaces -- there are
# .pak directories with them -- survive; a rename within one filesystem is atomic.
cd "$STAGE" || quarantine "Update staging failed. Nothing was changed."
find . -type d | while read -r dir; do
	mkdir -p "$ROOT/$dir"
done
find . -type f | while read -r file; do
	mv -f "$file" "$ROOT/$file"
done
cd /
rm -rf "$STAGE"

# Only now: everything that was in the archive is in place
rm -f "$ZIP"

sync
echo "Rebooting..."
sleep 2

shutdown -r

while true; do
	sync && reboot && sleep 10
done
