#!/usr/bin/env bash
# resolve-version.sh VERSION ACTION_PATH
#
# Print the scadman release tag the setup action installs: VERSION when it is given, or
# else the ref in the action's own path. The runner checks a downloaded action out at
# .../_actions/<owner>/<repo>/<ref>, so the setup action's path ends in
# .../scadman/<ref>/setup. That holds when the action runs nested in another composite
# action, where github.action_ref names the parent's ref, not this one. Only a release
# tag (vX.Y.Z...) resolves; a branch or a commit has no release to download.
set -euo pipefail

version=${1:-}
if [ -z "$version" ]; then
    path=$(printf '%s' "${2:-}" | tr '\\' '/')
    path=${path%/}
    path=${path%/setup}
    repo=$(printf '%s' "$path" | tr '[:upper:]' '[:lower:]')
    case "$repo" in
    */_actions/cameronbrooks11/scadman/*) version=${path##*/} ;;
    esac
fi

case "$version" in
v[0-9]*.[0-9]*.[0-9]*) printf '%s\n' "$version" ;;
*)
    echo "cannot tell which scadman release to install (ref '${version:-none}');" \
        "use the action at a release tag, or pass version: v0.1.0-alpha.N" >&2
    exit 1
    ;;
esac
