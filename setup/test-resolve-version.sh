#!/usr/bin/env bash
# The cases resolve-version.sh must get right. Run from the repository root.
set -uo pipefail
here=$(dirname "$0")
failed=0

check() { # check VERSION ACTION_PATH EXPECTED_EXIT EXPECTED_OUTPUT LABEL
    local out rc
    out=$(bash "$here/resolve-version.sh" "$1" "$2" 2>/dev/null)
    rc=$?
    if [ "$rc" = "$3" ] && [ "$out" = "$4" ]; then
        echo "ok   $5"
    else
        echo "FAIL $5: exit $rc, output '$out'"
        failed=1
    fi
}

check "v0.1.0-alpha.7" "/x" 0 "v0.1.0-alpha.7" "an explicit version wins"
check "" "/home/runner/work/_actions/CameronBrooks11/scadman/v0.1.0-alpha.7/setup" 0 "v0.1.0-alpha.7" "the tag comes from a downloaded action's path"
check "" 'D:\a\_actions\CameronBrooks11\scadman\v0.1.0-alpha.7\setup' 0 "v0.1.0-alpha.7" "a Windows path"
check "" "/home/runner/work/_actions/cameronbrooks11/SCADMAN/v1.2.3/setup/" 0 "v1.2.3" "owner and repo in any case, with a trailing slash"
check "" "/home/runner/work/scadman/scadman/setup" 1 "" "a local ./setup names no release"
check "" "/home/runner/work/_actions/CameronBrooks11/scadman/main/setup" 1 "" "a branch is refused"
check "" "/home/runner/work/_actions/CameronBrooks11/scadman/0c9e7c7a1b2c3d4e5f60718293a4b5c6d7e8f901/setup" 1 "" "a commit is refused"
check "" "/home/runner/work/_actions/someone/other/v1.0.0/setup" 1 "" "another repository's path is refused"
exit $failed
