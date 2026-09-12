#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "生成 Xcode 工程必须使用 macOS。" >&2
  exit 1
fi
command -v xcodegen >/dev/null || { echo "请先安装 XcodeGen：brew install xcodegen" >&2; exit 1; }

bash "$SCRIPT_DIR/build-xcframework.sh"
cd "$PROJECT_ROOT/ios"
xcodegen generate
echo "请打开 $PROJECT_ROOT/ios/CameraBridge.xcodeproj"
