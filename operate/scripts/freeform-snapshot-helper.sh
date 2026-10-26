#!/bin/bash
# Package an independently TCC-authorized, on-demand snapshot helper.
set -euo pipefail
PACKAGE=$(cd "$(dirname "$0")/.." && pwd)
LABEL=com.symaira.operate.freeform-snapshot
NAME='Symaira Operate Freeform Snapshot.app'
DEST="$HOME/Applications/Symaira"
PLIST_DIR="$HOME/Library/LaunchAgents"
SIGN=''
UNINSTALL=false
STAGED=false
while [[ $# -gt 0 ]]; do
    case "$1" in
        --sign) [[ $# -ge 2 ]] || exit 2; SIGN=$2; shift 2 ;;
        --dest) [[ $# -ge 2 ]] || exit 2; DEST=$2; STAGED=true; shift 2 ;;
        --uninstall) UNINSTALL=true; shift ;;
        *) printf 'Usage: %s [--sign IDENTITY] [--dest DIR] [--uninstall]\n' "$0" >&2; exit 2 ;;
    esac
done
[[ "$DEST" = /* && "$DEST" != / ]] || { printf 'Destination must be an absolute non-root directory.\n' >&2; exit 2; }
if $STAGED; then PLIST_DIR="$DEST/LaunchAgents"; fi
APP="$DEST/$NAME"
PLIST="$PLIST_DIR/$LABEL.plist"
if $UNINSTALL; then
    if ! $STAGED; then
        # A missing job is already unregistered. Other failures must not be hidden.
        if launchctl print "gui/$(id -u)/$LABEL" >/dev/null 2>&1; then
            launchctl bootout "gui/$(id -u)/$LABEL"
        fi
    fi
    rm -rf -- "$APP"
    rm -f -- "$PLIST"
    printf 'Removed helper bundle and plist. Cached snapshots are retained; delete them explicitly if desired.\n'
    exit 0
fi
BUILD=(swift)
if [[ -x /Users/daniel/.local/bin/dev-external ]]; then
    /Users/daniel/.local/bin/dev-external --status
    BUILD=(/Users/daniel/.local/bin/dev-external swift)
fi
"${BUILD[@]}" build --package-path "$PACKAGE" -c release --product symoperate-freeform-snapshot
BIN_DIR=$("${BUILD[@]}" build --package-path "$PACKAGE" -c release --show-bin-path)
mkdir -p "$DEST" "$PLIST_DIR"
# Prepare a fresh bundle before replacing an installed version; never modify a running binary in-place.
STAGE=$(mktemp -d "$DEST/.freeform-package.XXXXXX")
trap 'rm -rf -- "$STAGE"' EXIT
mkdir -p "$STAGE/$NAME/Contents/MacOS"
cp "$BIN_DIR/symoperate-freeform-snapshot" "$STAGE/$NAME/Contents/MacOS/"
python3 - "$STAGE/$NAME/Contents/Info.plist" "$PLIST" "$APP" "$LABEL" <<'PY'
import os, plistlib, sys
info, agent, app, label = sys.argv[1:]
with open(info, 'wb') as f:
    plistlib.dump(dict(CFBundleIdentifier=label, CFBundleName='Symaira Operate Freeform Snapshot',
        CFBundleExecutable='symoperate-freeform-snapshot', CFBundlePackageType='APPL',
        CFBundleVersion='1', CFBundleShortVersionString='1.0', LSMinimumSystemVersion='26.0',
        LSBackgroundOnly=True, LSUIElement=True), f)
# The agent plist is written only after packaging/signing succeeds below.
PY
if [[ -n "$SIGN" ]]; then
    codesign --force --sign "$SIGN" --options runtime --timestamp "$STAGE/$NAME"
    codesign --verify --strict "$STAGE/$NAME"
else
    printf 'Unsigned development bundle. Use --sign with a stable Developer ID identity for FDA.\n' >&2
fi
rm -rf -- "$APP"
mv "$STAGE/$NAME" "$APP"
python3 - "$PLIST" "$APP" "$LABEL" <<'PY'
import os, plistlib, sys
path, app, label = sys.argv[1:]
data = dict(Label=label, ProgramArguments=[app + '/Contents/MacOS/symoperate-freeform-snapshot'],
            AssociatedBundleIdentifiers=[label])
with open(path + '.tmp', 'wb') as f:
    plistlib.dump(data, f)
os.chmod(path + '.tmp', 0o600)
os.replace(path + '.tmp', path)
PY
printf 'Bundle: %s\nLaunchAgent: %s\n' "$APP" "$PLIST"
if $STAGED; then
    printf 'Staging only: no installation or launchctl operation performed.\n'
else
    printf 'Run once yourself in Terminal (not from an agent host):\n'
    printf 'launchctl bootstrap gui/$(id -u) %q\n' "$PLIST"
fi
printf 'System Settings > Privacy & Security > Full Disk Access: add and enable\n%s\n' "$APP"
printf 'Do not launch with open/AppleScript or as a child of the agent. launchd starts this one-shot helper on demand.\n'
