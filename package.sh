#!/usr/bin/env bash
# package.sh — Create macOS .app bundle after release build
set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "$0")" && pwd)"
TARGET="${1:-}"

if [[ -n "$TARGET" ]]; then
  RELEASE_DIR="$PROJECT_DIR/target/$TARGET/release"
else
  RELEASE_DIR="$PROJECT_DIR/target/release"
fi

BINARY="$RELEASE_DIR/lantype"
APP_DIR="$RELEASE_DIR/LanType.app"

echo "==> Cleaning old .app bundle..."
rm -rf "$APP_DIR"

echo "==> Creating .app structure..."
mkdir -p "$APP_DIR/Contents/MacOS"
mkdir -p "$APP_DIR/Contents/Resources"

echo "==> Copying binary..."
cp "$BINARY" "$APP_DIR/Contents/MacOS/lantype"
chmod +x "$APP_DIR/Contents/MacOS/lantype"

echo "==> Copying resources..."
cp "$PROJECT_DIR/Info.plist" "$APP_DIR/Contents/"
cp "$PROJECT_DIR/icons/icon.icns" "$APP_DIR/Contents/Resources/"

echo "==> Done: $APP_DIR"
echo "    Run with: open $APP_DIR"
