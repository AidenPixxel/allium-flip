#!/bin/sh
# Fails if any of Allium's RetroArch patches is missing from the built tree.
#
# Run inside the RetroArch-patch checkout after its `make all`. That Makefile applies patches in a
# for loop whose exit status is only the last patch's, so one that failed to apply mid-list would
# otherwise go unnoticed and the binary would ship without it -- and nothing downstream could tell.
# A patch that was applied reverses cleanly; one that was not does not.
#
# --force, not --batch: --batch lets patch decide a patch that will not reverse "looks reversed"
# and try it forwards instead, which succeeds on exactly the unpatched tree this is meant to catch.
set -u

build="$(sed -n 's/^BUILD_DIR[[:space:]]*[:?]*=[[:space:]]*//p' Makefile | head -1)"
build="${build:-build}"

status=0
for p in patches/*_allium_*.patch; do
	[ -f "$p" ] || continue
	if patch -R --dry-run --force -d "$build" -p1 < "$p" > /dev/null 2>&1; then
		echo "verified in built tree: $p"
	else
		echo "NOT APPLIED to built tree: $p" >&2
		status=1
	fi
done
exit $status
