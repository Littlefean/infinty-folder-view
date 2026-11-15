#!/bin/bash
set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$PROJECT_DIR"
if [[ "$(uname -s)" != "Darwin" ]]; then
  printf '这个 demo 的原生后端使用 Metal，目前只能在 macOS 上打包。\n' >&2
  exit 1
fi

BINARY="$PROJECT_DIR/target/release/infinity-markdown-skia"
if [[ ! -x "$BINARY" ]]; then
  if command -v cargo >/dev/null 2>&1; then
    CARGO="$(command -v cargo)"
  elif [[ -x "$HOME/.cargo/bin/cargo" ]]; then
    CARGO="$HOME/.cargo/bin/cargo"
  else
    printf '找不到 Cargo，请安装 Rust 或先提供 release 二进制。\n' >&2
    exit 1
  fi
  "$CARGO" build --release --locked
fi

APP_DIR="$PROJECT_DIR/dist/Markdown Atlas.app"
MACOS_DIR="$APP_DIR/Contents/MacOS"
RESOURCES_DIR="$APP_DIR/Contents/Resources"
mkdir -p "$MACOS_DIR" "$RESOURCES_DIR"
cp "$BINARY" "$MACOS_DIR/infinity-markdown-skia"
cp "$PROJECT_DIR/README.md" "$RESOURCES_DIR/README.md"

cat > "$MACOS_DIR/markdown-atlas" <<'LAUNCHER'
#!/bin/bash
set -euo pipefail
MACOS_DIR="$(cd "$(dirname "$0")" && pwd)"
export MARKDOWN_ATLAS_DEMO_DIR="${MARKDOWN_ATLAS_DEMO_DIR:-$HOME/Library/Application Support/Markdown Atlas/demo-markdown}"
exec "$MACOS_DIR/infinity-markdown-skia" "$@"
LAUNCHER

cat > "$APP_DIR/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleIdentifier</key>
  <string>com.fittencode.markdown-atlas</string>
  <key>CFBundleName</key>
  <string>Markdown Atlas</string>
  <key>CFBundleDisplayName</key>
  <string>Markdown Atlas</string>
  <key>CFBundleExecutable</key>
  <string>markdown-atlas</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>0.1.0</string>
  <key>CFBundleVersion</key>
  <string>1</string>
  <key>NSHighResolutionCapable</key>
  <true/>
  <key>NSSupportsAutomaticGraphicsSwitching</key>
  <true/>
  <key>LSApplicationCategoryType</key>
  <string>public.app-category.developer-tools</string>
</dict>
</plist>
PLIST

chmod +x "$MACOS_DIR/markdown-atlas" "$MACOS_DIR/infinity-markdown-skia"
/usr/bin/plutil -lint "$APP_DIR/Contents/Info.plist"
/usr/bin/codesign --force --deep --sign - "$APP_DIR"
printf '\n应用已生成：%s\n双击即可启动，无需从应用内调用 Cargo。\n' "$APP_DIR"
