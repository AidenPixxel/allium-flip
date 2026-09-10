#!/bin/sh
mydir=`dirname "$0"`
export HOME=$mydir
export PATH=$mydir/bin:$PATH
export LD_LIBRARY_PATH=$mydir/libs:$LD_LIBRARY_PATH
cd $mydir

# Video decoding wants the CPU held at full speed, but the governor is global, so what is borrowed
# here has to be given back. Allium's daemon used to do that on the way back to the launcher; now
# that nothing in Allium writes cpufreq at all, this is the only thing that will. Only pin it if
# the current value could be read, so a device with no cpufreq is left alone rather than having
# the node invented underneath it.
GOVERNOR="${FFPLAY_GOVERNOR-/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor}"
previous=$(cat "$GOVERNOR" 2> /dev/null)

restore() {
	rm -f /tmp/stay_awake
	if [ -n "$previous" ]; then
		echo "$previous" > "$GOVERNOR" 2> /dev/null
	fi
}

if [ -n "$previous" ]; then
	echo performance > "$GOVERNOR" 2> /dev/null
fi
touch /tmp/stay_awake
trap restore EXIT

# ffplay goes in the background so this shell stays able to act on the SIGTERM alliumd sends when
# a video is quit rather than left to finish: a shell blocked on a foreground child does not run
# its traps until that child exits, which for a video still playing is never -- and the governor
# would stay pinned, which is the whole thing this is here to prevent.
ffplay -autoexit -vf "hflip,vflip" -i "$1" &
pid=$!
trap 'kill -TERM "$pid" 2> /dev/null' INT TERM
wait "$pid"
exit $?
