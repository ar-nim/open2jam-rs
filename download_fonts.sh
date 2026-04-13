#!/usr/bin/env bash
# Download and install bundled fonts for the menu app.
# Fonts are placed in `crates/menu/assets/`.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ASSETS_DIR="$SCRIPT_DIR/crates/menu/assets"
mkdir -p "$ASSETS_DIR"

echo "=== Downloading Bundled Fonts ==="

# Inter font (Latin/Western text) - v4.1
# https://rsms.me/inter/
INTER_URL="https://github.com/rsms/inter/releases/download/v4.1/Inter-4.1.zip"
INTER_TMP=$(mktemp -d)
echo "Downloading Inter font..."
curl -sL "$INTER_URL" -o "$INTER_TMP/inter.zip"
unzip -q -o "$INTER_TMP/inter.zip" -d "$INTER_TMP"
# Use InterVariable.ttf (variable font, excellent quality)
cp "$INTER_TMP/InterVariable.ttf" "$ASSETS_DIR/Inter-Regular.ttf"
echo "  ✓ Inter-Regular.ttf installed"

# Noto Sans SC (CJK characters) - Simplified Chinese
# https://fonts.google.com/noto/specimen/Noto+Sans+SC
NOTO_URL="https://github.com/notofonts/noto-cjk/releases/download/Sans2.004/08_NotoSansCJKsc.zip"
NOTO_TMP=$(mktemp -d)
echo "Downloading Noto Sans SC..."
curl -sL "$NOTO_URL" -o "$NOTO_TMP/noto.zip"
unzip -q -o "$NOTO_TMP/noto.zip" -d "$NOTO_TMP"
# Find the OTF font file
find "$NOTO_TMP" -name "NotoSansCJKsc*.otf" -o -name "NotoSansCJKsc*.ttf" | head -1 | xargs -I {} cp {} "$ASSETS_DIR/NotoSansSC-Regular.ttf"
echo "  ✓ NotoSansSC-Regular.ttf installed"

# Cleanup
rm -rf "$INTER_TMP" "$NOTO_TMP"

echo ""
echo "=== Font Installation Complete ==="
echo "Fonts installed in: $ASSETS_DIR"
echo ""
echo "Font files:"
ls -lh "$ASSETS_DIR"
echo ""
echo "You can now run the menu app:"
echo "  cargo run -p open2jam-rs-menu"
