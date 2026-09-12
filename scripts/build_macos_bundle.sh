#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

echo "=== Building SpotyBurn macOS Application (.app & .dmg) ==="

# 1. Compile Release Binary
echo "[1/4] Compiling Rust release binary..."
cargo build --release --manifest-path "${ROOT_DIR}/src-tauri/Cargo.toml"

BINARY_PATH="${ROOT_DIR}/src-tauri/target/release/spotyburn"
if [ ! -f "${BINARY_PATH}" ]; then
    echo "Error: Binary not found at ${BINARY_PATH}"
    exit 1
fi

BUNDLE_DIR="${ROOT_DIR}/target/bundle/osx"
APP_DIR="${BUNDLE_DIR}/SpotyBurn.app"
CONTENTS_DIR="${APP_DIR}/Contents"
MACOS_DIR="${CONTENTS_DIR}/MacOS"
RESOURCES_DIR="${CONTENTS_DIR}/Resources"

echo "[2/4] Assembling SpotyBurn.app bundle structure..."
rm -rf "${APP_DIR}"
mkdir -p "${MACOS_DIR}" "${RESOURCES_DIR}"

# Copy binary
cp "${BINARY_PATH}" "${MACOS_DIR}/spotyburn"
chmod +x "${MACOS_DIR}/spotyburn"

# Copy App Icon
ICON_SRC="${ROOT_DIR}/src-tauri/icons/icon.icns"
if [ -f "${ICON_SRC}" ]; then
    cp "${ICON_SRC}" "${RESOURCES_DIR}/icon.icns"
fi

# Create Info.plist
cat <<'EOF' > "${CONTENTS_DIR}/Info.plist"
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleName</key>
    <string>SpotyBurn</string>
    <key>CFBundleDisplayName</key>
    <string>SpotyBurn</string>
    <key>CFBundleIdentifier</key>
    <string>com.spotyburn.app</string>
    <key>CFBundleVersion</key>
    <string>0.1.0</string>
    <key>CFBundleShortVersionString</key>
    <string>0.1.0</string>
    <key>CFBundleExecutable</key>
    <string>spotyburn</string>
    <key>CFBundleIconFile</key>
    <string>icon.icns</string>
    <key>LSMinimumSystemVersion</key>
    <string>10.15</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSSupportsAutomaticGraphicsSwitching</key>
    <true/>
</dict>
</plist>
EOF

# 3. Ad-hoc Code Signing for Local Gatekeeper
echo "[3/4] Ad-hoc signing SpotyBurn.app..."
if command -v codesign &>/dev/null; then
    codesign -s - --force --deep "${APP_DIR}"
    echo "Successfully signed with ad-hoc signature."
fi

# 4. Create DMG Installer
echo "[4/4] Generating installable SpotyBurn.dmg..."
DMG_STAGING="${ROOT_DIR}/target/bundle/dmg_staging"
rm -rf "${DMG_STAGING}"
mkdir -p "${DMG_STAGING}"

cp -R "${APP_DIR}" "${DMG_STAGING}/SpotyBurn.app"
ln -s /Applications "${DMG_STAGING}/Applications"

DMG_OUTPUT="${BUNDLE_DIR}/SpotyBurn.dmg"
rm -f "${DMG_OUTPUT}"

hdiutil create \
    -volname "SpotyBurn" \
    -srcfolder "${DMG_STAGING}" \
    -ov \
    -format UDZO \
    "${DMG_OUTPUT}"

rm -rf "${DMG_STAGING}"

echo "=== Build Complete! ==="
echo "App Bundle:  ${APP_DIR}"
echo "DMG Package: ${DMG_OUTPUT}"
ls -lh "${DMG_OUTPUT}"
