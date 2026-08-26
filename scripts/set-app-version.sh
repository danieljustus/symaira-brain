#!/bin/sh
set -eu

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PROJECT_YML="$REPO_ROOT/project.yml"

if [ ! -f "$PROJECT_YML" ]; then
  echo "error: project.yml not found at $PROJECT_YML" >&2
  exit 1
fi

TAG="${1:-}"
if [ -z "$TAG" ]; then
  if ! TAG="$(git -C "$REPO_ROOT" describe --tags --abbrev=0 2>/dev/null)" || [ -z "$TAG" ]; then
    echo "error: no git tag found to derive app version" >&2
    exit 1
  fi
fi

VERSION="${TAG#v}"

# Validate SemVer format: MAJOR.MINOR.PATCH with optional prerelease/build metadata
if ! echo "$VERSION" | grep -Eq '^[0-9]+(\.[0-9]+)*([+-].*)?$'; then
  echo "error: invalid version format '$VERSION' (expected SemVer e.g. 0.7.2)" >&2
  exit 1
fi

# Rewrite MARKETING_VERSION lines in project.yml idempotently
TMP_FILE="$(mktemp "${PROJECT_YML}.tmp.XXXXXX")"
sed "s/MARKETING_VERSION: \".*\"/MARKETING_VERSION: \"$VERSION\"/g" "$PROJECT_YML" > "$TMP_FILE"
mv "$TMP_FILE" "$PROJECT_YML"

echo "Updated MARKETING_VERSION in project.yml to $VERSION"
