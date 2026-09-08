#!/bin/sh
#
# Pushes an Allium build onto the handheld over Wi-Fi, so updating needs no SD card.
#
# Fetches a release here and uploads it to the file server the device already runs, landing it at
# /mnt/SDCARD/allium-ota.zip. The boot script picks it up from there and installs it on the next
# start.
#
# Why not let the device fetch it itself: its reqwest is built with no TLS backend at all, so it
# cannot reach https://api.github.com. Downloading here, where TLS works, and uploading over plain
# HTTP on the LAN sidesteps that entirely.
#
# The archive is checked before it is uploaded, so a download cut short never reaches the device.
# The install script verifies it again on the device before extracting.
#
# Usage:
#   scripts/push-update.sh <device-ip> [zip]
#
#   scripts/push-update.sh 192.168.1.42
#   scripts/push-update.sh 192.168.1.42 ~/Downloads/allium-armv7-unknown-linux-gnueabihf.zip
#
# Needs Wi-Fi on and Settings > Wi-Fi > Web File Explorer on. Note that the device pings 1.1.1.1
# before starting that server, so it will not come up on a LAN with no route to the internet.

set -eu

# Has to match RELEASE_FILE in crates/allium-launcher/src/ota.rs and the name CI publishes
ASSET="allium-armv7-unknown-linux-gnueabihf.zip"

# The name the boot script looks for, per static/.tmp_update/updater
OTA_NAME="allium-ota.zip"

REPO="${REPO:-AidenPixxel/allium-flip}"

DEVICE="${1:-}"
ZIP="${2:-}"

if [ -z "$DEVICE" ]; then
	echo "usage: $0 <device-ip> [zip]" >&2
	echo "  the IP is on the handheld under Settings > Wi-Fi > IP Address" >&2
	exit 2
fi

for tool in curl unzip; do
	command -v "$tool" > /dev/null 2>&1 || {
		echo "$tool is required but not installed." >&2
		exit 1
	}
done

TMP=""
cleanup() {
	[ -n "$TMP" ] && rm -f "$TMP"
}
trap cleanup EXIT

if [ -n "$ZIP" ]; then
	[ -f "$ZIP" ] || {
		echo "No such file: $ZIP" >&2
		exit 1
	}
	SRC="$ZIP"
	echo "Using $SRC"
else
	# GitHub's documented permalink for a release asset, so this needs no API call and no JSON
	# parsing -- curl just follows the redirect.
	TMP="$(mktemp -t allium-ota.XXXXXX)"
	SRC="$TMP"
	echo "Fetching the latest release from $REPO ..."
	curl -fL --progress-bar -o "$SRC" \
		"https://github.com/$REPO/releases/latest/download/$ASSET" || {
		echo "Download failed. Is there a published release with an $ASSET asset?" >&2
		exit 1
	}
fi

echo "Checking the archive ..."

# `unzip -t` reads and CRC-checks every entry, so this catches a download cut short and quiet
# corruption alike -- a stronger check than merely opening the archive.
unzip -t "$SRC" > /dev/null 2>&1 || {
	echo "The archive is incomplete or corrupt. Nothing was uploaded." >&2
	exit 1
}

# An Allium build, not just any archive. Extracting something else over the SD root would do real
# damage.
NAMES="$(unzip -Z1 "$SRC" 2> /dev/null || unzip -l "$SRC")"
for want in ".allium/" ".tmp_update/"; do
	printf '%s\n' "$NAMES" | grep -q "$want" || {
		echo "Archive has no $want -- this does not look like an Allium build." >&2
		exit 1
	}
done

echo "Uploading to http://$DEVICE/$OTA_NAME ..."
curl -f --progress-bar -T "$SRC" "http://$DEVICE/$OTA_NAME" || {
	echo "Upload failed. Check the IP, and that Settings > Wi-Fi > Web File Explorer is on." >&2
	exit 1
}

echo
echo "Uploaded. Restart the handheld to install it."
