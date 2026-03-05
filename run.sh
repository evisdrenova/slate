#!/bin/bash
# Build and run Slate as a macOS .app bundle (with dock icon)
set -e

cargo build 2>&1

APP_DIR="target/Slate.app"
mkdir -p "$APP_DIR/Contents/MacOS"
mkdir -p "$APP_DIR/Contents/Resources"
cp assets/icon.icns "$APP_DIR/Contents/Resources/AppIcon.icns"
cp target/debug/slate "$APP_DIR/Contents/MacOS/slate"

# Kill any existing instance and wait for it to exit
pkill -f "Slate.app/Contents/MacOS/slate" 2>/dev/null && sleep 0.5 || true

open "$APP_DIR"
