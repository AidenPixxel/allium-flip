#!/bin/sh
# Adapted from DotUI-X: https://github.com/anzz1/DotUI-X/blob/master/extras/Tools/WiFi.pak/wifion.sh
#
# Safe to run again while Wi-Fi is already up: every run leaves exactly one wpa_supplicant and one
# udhcpc. It used to start another pair on top of whatever was running -- and the settings screen
# re-runs this on every network-name or password edit -- so an ordinary first-time setup ended with
# three supplicants fighting over wlan0 while the radio rail was power-cycled underneath them.

if ! grep -q 8188fu /proc/modules; then
	insmod /mnt/SDCARD/.tmp_update/8188fu.ko
fi
ifconfig lo up

# Stop whatever an earlier run left behind before starting our own
killall wpa_supplicant udhcpc > /dev/null 2>&1
sleep 0.2

# Only power the rail when the interface will not come up without it. Cycling it on every run is
# what made the radio thrash.
if ! ifconfig wlan0 up > /dev/null 2>&1; then
	/customer/app/axp_test wifion
	sleep 2
	ifconfig wlan0 up
fi

/customer/app/wpa_supplicant -B -D nl80211 -iwlan0 -c /appconfigs/wpa_supplicant.conf
ln -sf /dev/null /tmp/udhcpc.log
udhcpc -i wlan0 -s /etc/init.d/udhcpc.script > /dev/null 2>&1 &
