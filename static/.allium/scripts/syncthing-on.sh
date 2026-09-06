#!/bin/sh

dir=$(dirname "$0")
if "$dir"/wait-for-wifi.sh; then
    cd /mnt/SDCARD/ || exit
    if [ ! -d "/mnt/SDCARD/.syncthing/config" ]; then
        mkdir -p "/mnt/SDCARD/.syncthing/config"
    fi
    "$ROOT/.allium/bin/syncthing" --gui-address=0.0.0.0:8384 --home=/mnt/SDCARD/.syncthing/config/ > /mnt/SDCARD/.syncthing/serve.log 2>&1 &
    exit 0
fi

exit 1
