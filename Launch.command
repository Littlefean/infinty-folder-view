#!/bin/bash
set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$PROJECT_DIR"
BINARY="$PROJECT_DIR/target/release/infinity-markdown-skia"

if [[ ! -x "$BINARY" ]]; then
  if command -v cargo >/dev/null 2>&1; then
    CARGO="$(command -v cargo)"
  elif [[ -x "$HOME/.cargo/bin/cargo" ]]; then
    CARGO="$HOME/.cargo/bin/cargo"
  else
    printf '找不到 Cargo。请先安装 Rust，再双击启动。\n' >&2
    exit 1
  fi
  printf '首次启动，正在编译 Rust + Skia demo…\n'
  "$CARGO" build --release --locked
fi

exec "$BINARY" "$@"
