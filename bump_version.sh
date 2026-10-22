#!/usr/bin/env bash
set -euo pipefail

# Auto version bump with codename
# Usage: ./bump_version.sh [major|minor|patch] [codename]

set -euo pipefail

# Moon/celestial codenames
CODENAMES=(
    "Atlas" "Phoebe" "Selene" "Tsuki" "Hoshi" "Sakura" 
    "Astraea" "Yuzuki" "Rigel" "Spica" "Lyra" "Mizar" 
    "Alcor" "Polaris" "Vega" "Sirius" "Betelgeuse" "Deneb" "Procyon"
)

# Get current version from Cargo.toml
CURRENT_VERSION=$(grep '^version = ' Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')
echo "Current version: $CURRENT_VERSION"

IFS='.' read -r MAJOR MINOR PATCH <<< "$CURRENT_VERSION"

# Determine bump type
BUMP_TYPE="${1:-patch}"
CODENAME="${2:-}"

case "$BUMP_TYPE" in
    major) MAJOR=$((MAJOR + 1)); MINOR=0; PATCH=0 ;;
    minor) MINOR=$((MINOR + 1)); PATCH=0 ;;
    patch) PATCH=$((PATCH + 1)) ;;
    *) echo "Usage: $0 [major|minor|patch] [codename]"; exit 1 ;;
esac

NEW_VERSION="${MAJOR}.${MINOR}.${PATCH}"

# Auto-generate codename if not provided
if [[ -z "$CODENAME" ]]; then
    CODENAME_IDX=$((PATCH % ${#CODENAMES[@]}))
    CODENAME=${CODENAMES[$CODENAME_IDX]}
fi

echo "New version: $NEW_VERSION"
echo "Codename: $CODENAME"

# Update all version files
sed -i "s/version = \"[0-9.]*\"/version = \"$NEW_VERSION\"/" Cargo.toml
sed -i "s/version: [0-9.]*+[0-9]*/version: ${NEW_VERSION}+${NEW_VERSION//./}/" ui/flutter/pubspec.yaml
sed -i "s/version=v[0-9.]*/version=v${NEW_VERSION}/" android/kernelsu-next/module.prop
sed -i "s/versionCode=[0-9]*/versionCode=$(date +%y%m%d)/" android/kernelsu-next/module.prop
sed -i "s/android:versionCode=\"[0-9]*\"/android:versionCode=\"$(date +%y%m%d)\"/" android/package/AndroidManifest.xml
sed -i "s/android:versionName=\"[0-9.]*\"/android:versionName=\"${NEW_VERSION}\"/" android/package/AndroidManifest.xml
sed -i "s/const String dashVersionName = '[0-9.]*'/const String dashVersionName = '${NEW_VERSION}';/" ui/flutter/lib/release_identity.dart
sed -i "s/const int dashVersionCode = [0-9]*/const int dashVersionCode = $(date +%y%m%d)/" ui/flutter/lib/release_identity.dart
sed -i "s/const String dashReleaseCodename = '.*'/const String dashReleaseCodename = '${CODENAME}';/" ui/flutter/lib/release_identity.dart

# Update Cargo.lock
cargo update -p dash-essential-daemon -p dash-essential-host --offline

# Update CHANGELOG
DATE=$(date +%Y-%m-%d)
sed -i "1,/^## /s/^## /## ${NEW_VERSION}\n\n### Changed\n- Auto version bump with codename **${CODENAME}**.\n\n## /" CHANGELOG.md

# Build to verify
cargo build --release --target aarch64-unknown-linux-gnu -p dash-essential-daemon --lib
cargo check --release --target aarch64-linux-android -p dash-essential-daemon --lib --tests

echo "✅ Version bumped to ${NEW_VERSION} (${CODENAME})"
echo "Run: git add -A && git commit -m \"chore(release): ${NEW_VERSION} codename ${CODENAME}\" && git tag -a v${NEW_VERSION}-dash -m \"Dash Essential ${NEW_VERSION} (codename ${CODENAME})\" && git push origin main --follow-tags"
