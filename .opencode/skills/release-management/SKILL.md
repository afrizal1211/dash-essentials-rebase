---
name: release-management
description: Use when preparing, building, or publishing releases for dash-essentials. Covers version bumping, CHANGELOG format, GitHub release workflow, asset verification, and Telegram announcements.
---

# Release Management for dash-essentials

## Version Identity Files (6 files, must match)
| File | Version Format | Example |
|------|----------------|---------|
| `Cargo.toml` (workspace) | `version = "1.18.16"` | `version = "1.18.16"` |
| `Cargo.lock` | auto via `cargo update` | `version = "1.18.16"` |
| `ui/flutter/pubspec.yaml` | `version: 1.18.16+11816` | `version: 1.18.16+11816` |
| `android/kernelsu-next/module.prop` | `version=v1.18.16` + `versionCode=11816` | `version=v1.18.16` |
| `android/package/AndroidManifest.xml` | `android:versionName="1.18.16"` `android:versionCode="11816"` | |
| `ui/flutter/lib/release_identity.dart` | `dashVersionName = '1.18.16'` `dashVersionCode = 11816` `dashReleaseCodename = 'Tsuki'` | |

## CHANGELOG Format
```markdown
## 1.18.16

### Changed
- Change description
- Another change

### Fixed
- Bug fix description
```
- New version section **before** previous version
- Never replace previous heading
- "Pre-release." line → marks as GitHub pre-release

## Version Bump Script
```bash
# Update all 6 files
sed -i 's/version = "1.18.15"/version = "1.18.16"/' Cargo.toml
sed -i 's/version: 1.18.15+11815/version: 1.18.16+11816/' ui/flutter/pubspec.yaml
sed -i 's/version=v1.18.15/version=v1.18.16/' android/kernelsu-next/module.prop
sed -i 's/versionCode=11815/versionCode=11816/' android/kernelsu-next/module.prop
sed -i 's/versionName="1.18.15"/versionName="1.18.16"/' android/package/AndroidManifest.xml
sed -i 's/versionCode="11815"/versionCode="11816"/' android/package/AndroidManifest.xml
sed -i "s/const String dashVersionName = '1.18.15'/const String dashVersionName = '1.18.16'/" ui/flutter/lib/release_identity.dart
sed -i 's/const int dashVersionCode = 11815/const int dashVersionCode = 11816/' ui/flutter/lib/release_identity.dart
sed -i "s/const String dashReleaseCodename = 'Astraea'/const String dashReleaseCodename = 'Tsuki'/" ui/flutter/lib/release_identity.dart

# Update Cargo.lock
cargo update -p dash-essential-daemon -p dash-essential-host --offline
```

## CHANGELOG Update
```bash
# Insert new section before previous version
sed -i '0,/^## 1.18.15/s//## 1.18.16\n\n### Changed\n- New feature\n\n## 1.18.15/' CHANGELOG.md
```

## Git Operations
```bash
git add -A
git -c user.name="afrizal1211" -c user.email="344513460+afrizal1211@users.noreply.github.com" commit -m "feat/release: 1.18.16"
git tag -a v1.18.16-dash -m "Dash Essential 1.18.16 (codename Tsuki)"
git push origin main --follow-tags
```

## CI Workflow (build-dash-essential-module.yml)
Triggers on tag `v*`. Produces 4 assets:
1. `Dash-Essential-KernelSU-Next-Magisk-vX.Y.Z.zip`
2. `Dash-Essential-KernelSU-Next-Magisk-vX.Y.Z.zip.sha256`
3. `Dash-Essential.apk`
4. `Dash-Essential.apk.sha256`

Release is **pre-release** if CHANGELOG contains "Pre-release." (case-insensitive).

## Telegram Announcement
Auto-sent by CI (requires `TELEGRAM_BOT_TOKEN` + `TELEGRAM_CHAT_ID` secrets).
Short format: build name, codename, key changes, link.

## Post-Release Verification
```bash
# Check release exists
curl -H "Authorization: token $GH_TOKEN" "https://api.github.com/repos/afrizal1211/dash-essentials-rebase/releases/tags/v1.18.16-dash"

# Verify assets
sha256sum -c Dash-Essential-KernelSU-Next-Magisk-*.zip.sha256

# Check release body
curl -H "Authorization: token $GH_TOKEN" "https://api.github.com/repos/afrizal1211/dash-essentials-rebase/releases/tags/v1.18.16-dash" | jq '.body'

# If draft, publish
curl -X PATCH -H "Authorization: token $GH_TOKEN" \
  "https://api.github.com/repos/afrizal1211/dash-essentials-rebase/releases/<release_id>" \
  -d '{"draft":false}'
```

## Stable Branch
```bash
git branch stable/1.18.16 v1.18.16-dash
git push origin stable/1.18.16
```

## Common Issues
- **Draft release**: CHANGELOG missing "Pre-release." but CI published as draft → PATCH `draft: false`
- **Missing assets**: Build failed → check CI logs
- **Telegram failed**: Release still published; announcement logged in CI
- **Version mismatch**: `tools/test-release-sync.sh` fails → check all 6 files