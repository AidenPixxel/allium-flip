#!/bin/sh
# Reports what the device's kernel actually offers for power management.
#
# Read-only: every line here reads a file or runs a reporting tool. Nothing is written, no service
# is started or stopped, and it is safe to run while a game is in the foreground.
#
# Why it exists: the battery work needs facts that are not published anywhere. Miyoo's kernel
# source covers the Mini and Mini+, and the Flip's own DTS and defconfig are not public -- so the
# frequency table, the core-voltage pin, the boot governor and whether the battery tool reports
# current all have to be read off the hardware. Answers from the Mini+ vendor tree are a guess
# about the Flip until this says otherwise.
#
# This lives in scripts/ and is not shipped to the card, so it runs from a computer. Turn on
# Settings -> Wi-Fi -> SSH Enabled, read the IP from the row above it, then pipe the script in --
# the report comes back to the computer, which is where you want it anyway:
#
#   ssh -o PreferredAuthentications=password -o PubkeyAuthentication=no \
#       -o HostKeyAlgorithms=+ssh-rsa root@<ip> 'sh -s' < scripts/probe-power.sh | tee probe.txt
#
# The password is blank -- press Enter. The extra options are for the device's old dropbear, which
# a current OpenSSH client otherwise refuses.
#
# Over telnet instead (Settings -> Wi-Fi -> Telnet Enabled, which drops straight to a root shell
# with no login): paste the contents of this file at the prompt.
#
# To keep it on the device, copy it to the card and run it from Apps -> Terminal:
#   sh /mnt/SDCARD/probe-power.sh
#
# Turn Wi-Fi and those services back off afterwards -- both are unauthenticated root on the LAN.
#
# `sh probe-power.sh idle` adds a ten-second sample of how often the CPU is being woken, which is
# the before/after measurement for the wakeup work. Put the device down and do not touch it while
# that part runs.

section() {
	echo
	echo "=== $1 ==="
}

# Print a file's contents, or say plainly that it is not there -- a missing node is itself an
# answer, and half this script is expected to be missing.
show() {
	if [ -r "$1" ]; then
		printf '%s: ' "$(basename "$1")"
		cat "$1" 2> /dev/null || echo "(unreadable)"
	else
		echo "$(basename "$1"): ABSENT"
	fi
}

# Run a command that may never exit. `axp_test` is the reason: Allium's own battery code kills it
# after 100ms rather than waiting, which says it does not reliably exit on its own.
run_briefly() {
	if command -v timeout > /dev/null 2>&1; then
		timeout "$1" sh -c "$2" 2>&1
	else
		sh -c "$2" 2>&1 &
		_pid=$!
		sleep "$1"
		kill "$_pid" 2> /dev/null
		wait "$_pid" 2> /dev/null
	fi
}

echo "Allium power probe"
echo "date:    $(date 2> /dev/null)"
echo "model:   $(strings -n 5 /customer/app/MainUI 2> /dev/null | grep -m1 MY || echo unknown)"
echo "uptime:  $(cat /proc/uptime 2> /dev/null)"
echo "kernel:  $(uname -a 2> /dev/null)"

section "CPU frequency"
# The whole 1.0 GHz "saver" idea rests on this table containing a step below 1100 MHz, and on the
# voltage dropping with it. The Mini+ vendor DTS says 400/600/800/1000 MHz at 0.9V and
# 1100/1200 MHz at 1.0V; if the Flip matches, capping the clock is a real saving rather than a
# no-op, because energy per unit of work goes with voltage squared.
for f in scaling_available_frequencies scaling_available_governors scaling_governor \
	scaling_cur_freq scaling_min_freq scaling_max_freq scaling_setspeed cpuinfo_cur_freq \
	cpuinfo_min_freq cpuinfo_max_freq; do
	show "/sys/devices/system/cpu/cpu0/cpufreq/$f"
done
echo "cores online: $(cat /sys/devices/system/cpu/online 2> /dev/null)"

section "Core voltage (the VID pin)"
# Miyoo's kernel is built with CONFIG_SS_VOLTAGE_CTRL=y and the Mini+ DTS declares a one-bit GPIO
# VID switching the core rail between 900mV and 1000mV, enabled at boot. These are the nodes that
# driver registers; if they are here, the voltage step is real on this device too.
for f in scaling_voltage voltage_available voltage_current vid_gpio_map; do
	found=$(find /sys -name "$f" 2> /dev/null | head -1)
	if [ -n "$found" ]; then
		echo "$f -> $found"
		cat "$found" 2> /dev/null
	else
		echo "$f: ABSENT"
	fi
done
show /sys/devices/system/cpu/cpufreq/temp_out

section "Kernel suspend"
# Expected to be absent: Miyoo builds with CONFIG_PM and CONFIG_SUSPEND unset, which is why every
# firmware on this hardware fakes sleep instead. If /sys/power/state exists after all, real
# suspend-to-RAM is on the table and the standby story changes completely.
if [ -d /sys/power ]; then
	ls /sys/power
	show /sys/power/state
else
	echo "/sys/power: ABSENT  (CONFIG_PM=n -- suspend-to-RAM is not available)"
fi
echo "cpuidle: $(ls /sys/devices/system/cpu/cpu0/cpuidle 2> /dev/null || echo ABSENT)"

section "Input devices"
# Which node is the lid? The daemon currently re-reads the hall sensor through sysfs twice a
# second, forever, in every process -- including while suspended -- purely because it has no event
# to wait on. If one of these devices reports the lid switch, that polling can go away entirely.
cat /proc/bus/input/devices 2> /dev/null || echo "(no /proc/bus/input/devices)"
show /sys/devices/soc0/soc/soc:hall-mh248/hallvalue

section "Battery tool"
# The question is whether this reports current or power, not just a percentage. If it does, every
# claim in the battery evaluation becomes something we measure instead of model -- it would be an
# on-device power meter, and worth more than the rest of this file put together.
if [ -x /customer/app/axp_test ]; then
	run_briefly 2 /customer/app/axp_test
else
	echo "/customer/app/axp_test: ABSENT"
fi
for f in /sys/class/power_supply/*/uevent; do
	[ -r "$f" ] && { echo "--- $f"; cat "$f"; }
done 2> /dev/null

section "Backlight"
for f in period duty_cycle enable; do
	show "/sys/class/pwm/pwmchip0/pwm0/$f"
done

section "What is running"
ps 2> /dev/null | head -40

if [ "$1" = "idle" ]; then
	section "Idle wakeups over 10s (do not touch the device)"
	# How busy the device is while apparently doing nothing. `intr` is the total interrupt count
	# and the timer interrupt dominates it; the idle jiffies come from /proc/stat's cpu line,
	# field 4. Run this before and after a wakeup change -- the numbers only mean something
	# compared against themselves, on the same screen, with the same thing in the foreground.
	before_intr=$(awk '/^intr/ {print $2}' /proc/stat 2> /dev/null)
	before_idle=$(awk '/^cpu / {print $5}' /proc/stat 2> /dev/null)
	sleep 10
	after_intr=$(awk '/^intr/ {print $2}' /proc/stat 2> /dev/null)
	after_idle=$(awk '/^cpu / {print $5}' /proc/stat 2> /dev/null)
	echo "interrupts in 10s: $((after_intr - before_intr))"
	echo "idle jiffies in 10s: $((after_idle - before_idle))  (a fully idle CPU is ~1000/s/core)"
	echo "context switches: $(awk '/^ctxt/ {print $2}' /proc/stat 2> /dev/null)"
else
	echo
	echo "(run with 'idle' as an argument to add a ten-second wakeup sample)"
fi

echo
echo "--- end of probe ---"
