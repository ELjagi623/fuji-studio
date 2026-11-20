#!/usr/bin/env bash
# Fuji Studio — build / run on macOS (Apple Silicon).
#
#   ./build-mac.sh          build the app  ->  target/release/bundle/macos/Fuji Studio.app (+ .dmg)
#   ./build-mac.sh dev      run in development mode (hot reload, terminal shows camera logs)
#   ./build-mac.sh cli      build the command-line probe  ->  target/release/fujictl
#
# First run installs nothing silently except Rust (rustup) if it is missing.
set -euo pipefail
cd "$(dirname "$0")"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "这个脚本只能在 Mac 上运行。" >&2
  exit 1
fi
if [[ "$(uname -m)" != "arm64" ]]; then
  echo "提示：当前不是 Apple 芯片（arm64），构建仍可进行，但这个版本只在 M 系列上测试。"
fi

# 1. Xcode Command Line Tools (C compiler for libusb, linker)
if ! xcode-select -p >/dev/null 2>&1; then
  echo "需要 Xcode 命令行工具。正在弹出安装窗口，装完后重新运行本脚本。"
  xcode-select --install || true
  exit 1
fi

# 2. Rust
if ! command -v cargo >/dev/null 2>&1; then
  if [[ -f "$HOME/.cargo/env" ]]; then
    # shellcheck disable=SC1091
    source "$HOME/.cargo/env"
  else
    echo "未找到 Rust，正在用 rustup 安装…"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    # shellcheck disable=SC1091
    source "$HOME/.cargo/env"
  fi
fi

mode="${1:-build}"

# The command-line probe only needs Rust.
if [[ "$mode" == "cli" ]]; then
  cargo build --release -p fujictl
  echo
  echo "完成：$(pwd)/target/release/fujictl"
  echo "试试：  target/release/fujictl devices   和   target/release/fujictl -v info"
  exit 0
fi

# 3. Node.js (app only)
if ! command -v node >/dev/null 2>&1; then
  echo "需要 Node.js 20 或更新版本：  brew install node   （或从 https://nodejs.org 下载）" >&2
  exit 1
fi

cd app
if [[ ! -d node_modules ]]; then
  npm ci
fi
# Official product photos for the sidebar (optional; the app works without them)
if [[ ! -d src/assets/cameras ]] || [[ -z "$(ls -A src/assets/cameras 2>/dev/null)" ]]; then
  npm run fetch-cameras || echo "（相机图片下载失败，可以忽略，界面会显示通用图标）"
fi

if [[ "$mode" == "dev" ]]; then
  exec npx tauri dev
fi

npx tauri build
echo
echo "完成。App：  $(cd .. && pwd)/target/release/bundle/macos/Fuji Studio.app"
ls ../target/release/bundle/dmg/*.dmg 2>/dev/null | sed 's/^/安装包：  /' || true
