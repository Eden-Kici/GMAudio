#!/usr/bin/env bash
# Builds GMAudio for Linux: a .deb package, an AppImage and a plain binary.
# Needs Rust, Node.js and the packages listed in the README.
set -euo pipefail
cd "$(dirname "$0")"
npm install
npm run tauri build -- --bundles deb,appimage
echo
echo "Done."
echo "  .deb:      target/release/bundle/deb/"
echo "  AppImage:  target/release/bundle/appimage/"
echo "  Binary:    target/release/gmaudio"
