#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
PACKAGE_ROOT="$PROJECT_ROOT/ios/Packages/CameraBridgeCorePackage"
ARTIFACT_ROOT="$PACKAGE_ROOT/Artifacts"
GENERATED_ROOT="$PACKAGE_ROOT/Sources/CameraBridgeCore/Generated"
BUILD_ROOT="$PROJECT_ROOT/target/apple-bindings"
DEVICE_TARGET="aarch64-apple-ios"
SIMULATOR_TARGET="aarch64-apple-ios-sim"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "生成 XCFramework 必须使用 macOS。" >&2
  exit 1
fi

for tool in cargo rustup xcodebuild; do
  command -v "$tool" >/dev/null || {
    echo "缺少构建工具：$tool" >&2
    exit 1
  }
done

cd "$PROJECT_ROOT"
export CARGO_TARGET_DIR="$PROJECT_ROOT/target"
rustup target add "$DEVICE_TARGET" "$SIMULATOR_TARGET"
cargo build --locked --release -p camera-ffi --target "$DEVICE_TARGET"
cargo build --locked --release -p camera-ffi --target "$SIMULATOR_TARGET"

TOOLS_ROOT="$PROJECT_ROOT/.tooling/apple-bindgen"
if [[ ! -x "$TOOLS_ROOT/bin/uniffi-bindgen" ]]; then
  cargo install uniffi --version 0.31.2 --features cli --locked --root "$TOOLS_ROOT"
fi

rm -rf "$BUILD_ROOT" "$ARTIFACT_ROOT/camera_ffiFFI.xcframework"
mkdir -p "$BUILD_ROOT/generated" "$BUILD_ROOT/headers" "$ARTIFACT_ROOT" "$GENERATED_ROOT"
"$TOOLS_ROOT/bin/uniffi-bindgen" generate \
  --library "$PROJECT_ROOT/target/$DEVICE_TARGET/release/libcamera_ffi.a" \
  --language swift \
  --out-dir "$BUILD_ROOT/generated"

HEADER_FILE="$(find "$BUILD_ROOT/generated" -maxdepth 1 -name '*FFI.h' -print -quit)"
MODULEMAP_FILE="$(find "$BUILD_ROOT/generated" -maxdepth 1 -name '*.modulemap' -print -quit)"
SWIFT_FILE="$(find "$BUILD_ROOT/generated" -maxdepth 1 -name '*.swift' -print -quit)"
test -n "$HEADER_FILE" && test -n "$MODULEMAP_FILE" && test -n "$SWIFT_FILE"

cp "$HEADER_FILE" "$BUILD_ROOT/headers/"
cp "$MODULEMAP_FILE" "$BUILD_ROOT/headers/module.modulemap"
cp "$SWIFT_FILE" "$GENERATED_ROOT/camera_ffi.swift"

xcodebuild -create-xcframework \
  -library "$PROJECT_ROOT/target/$DEVICE_TARGET/release/libcamera_ffi.a" \
  -headers "$BUILD_ROOT/headers" \
  -library "$PROJECT_ROOT/target/$SIMULATOR_TARGET/release/libcamera_ffi.a" \
  -headers "$BUILD_ROOT/headers" \
  -output "$ARTIFACT_ROOT/camera_ffiFFI.xcframework"

echo "已生成 $ARTIFACT_ROOT/camera_ffiFFI.xcframework"
