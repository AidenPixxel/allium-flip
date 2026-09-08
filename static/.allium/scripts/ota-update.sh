#!/bin/sh

ZIP=/mnt/SDCARD/allium-ota.zip

# Check if update file exists
if [ ! -f "$ZIP" ]; then
	echo "Update file not found at $ZIP" >&2
	exit 1
fi

# Check available space
available_space=$(df -m /mnt/SDCARD | tail -1 | awk '{print $4}')
if [ "$available_space" -lt 300 ]; then
	echo "You need 300MB of free space to update Allium." >&2
	exit 1
fi

show --clear

# Prove the archive is whole before overwriting a live system with it. A download cut short is the
# common failure and it is cheap to catch: a zip keeps its index at the end of the file, so an
# incomplete one cannot even be listed. This costs a read rather than a write.
if ! miniunz -l "$ZIP" > /dev/null 2>&1; then
	echo "Update archive is incomplete or corrupt, not extracting." >&2
	# Moved aside rather than left in place. `updater` only checks that the file exists, so
	# leaving it would announce an update and fail again on every boot from here on.
	mv -f "$ZIP" "$ZIP.bad"
	say "Update file was incomplete. Nothing was changed."
	sleep 3
	exit 1
fi

say "Updating Allium. Please wait..."

# Extract update
if ! miniunz -x -o "$ZIP" -d "/mnt/SDCARD/"; then
	echo "Update extraction failed." >&2
	# Same reasoning: an archive that verified but still failed to extract means the card is full
	# or failing, and retrying it every boot would only loop.
	mv -f "$ZIP" "$ZIP.bad"
	exit 1
fi

# Remove update file
rm -f "$ZIP"

sync
echo "Rebooting..."
sleep 2

shutdown -r

while true; do
	sync && reboot && sleep 10
done
