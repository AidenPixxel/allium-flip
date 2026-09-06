#!/bin/sh

set_snd_level() {
    local start_time
    local elapsed_time

    start_time=$(date +%s)
    while [ ! -e /proc/mi_modules/mi_ao/mi_ao0 ]; do
        sleep 0.1
        elapsed_time=$(($(date +%s) - start_time))
        if [ "$elapsed_time" -ge 30 ]; then
            return 1
        fi
    done

    vol=$(cat /tmp/volume 2>/dev/null)
    [ -n "$vol" ] || vol=-9

    echo "set_ao_mute 0" >/proc/mi_modules/mi_ao/mi_ao0
    echo "set_ao_volume 0 ${vol}dB" >/proc/mi_modules/mi_ao/mi_ao0
    echo "set_ao_volume 1 ${vol}dB" >/proc/mi_modules/mi_ao/mi_ao0

}

set_snd_level &

if [ -f /mnt/SDCARD/.tmp_update/script/stop_audioserver.sh ]; then
    /mnt/SDCARD/.tmp_update/script/stop_audioserver.sh
fi

"$ROOT"/.allium/cores/ffplay/launch_ffplay.sh "$@"

if [ -f /mnt/SDCARD/.tmp_update/script/start_audioserver.sh ]; then
    /mnt/SDCARD/.tmp_update/script/start_audioserver.sh
fi
