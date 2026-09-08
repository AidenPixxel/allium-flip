#!/bin/sh

dir=$(dirname "$0")
if "$dir"/wait-for-wifi.sh; then
    cd /mnt/SDCARD/ || exit
    # Backgrounded like the other services, so the caller's wait returns. This used `&>`, which
    # is not POSIX: busybox sh reads it as `&` followed by a stray redirect.
    "$ROOT/.allium/bin/dufs" --allow-all --bind 0.0.0.0 --port 80 > /dev/null 2>&1 &
    exit 0
fi

exit 1
