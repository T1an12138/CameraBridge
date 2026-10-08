#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"
: "${ANDROID_NDK_HOME:?Set ANDROID_NDK_HOME to an Android NDK r28b directory}"
case "$(uname -s)" in
  Linux*) HOST=linux-x86_64 ;;
  Darwin*) HOST=darwin-x86_64 ;;
  *) echo 'On Windows use scripts/android/build-native.ps1' >&2; exit 1 ;;
esac
LLVM="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/$HOST/bin"
export CARGO_TARGET_DIR="$ROOT/target"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$LLVM/aarch64-linux-android29-clang"
export CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER="$LLVM/x86_64-linux-android29-clang"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS='-C link-arg=-Wl,-z,max-page-size=16384'
export CARGO_TARGET_X86_64_LINUX_ANDROID_RUSTFLAGS='-C link-arg=-Wl,-z,max-page-size=16384'
rustup target add aarch64-linux-android x86_64-linux-android
for mapping in 'aarch64-linux-android:arm64-v8a' 'x86_64-linux-android:x86_64'; do
  target="${mapping%:*}"
  abi="${mapping#*:}"
  cargo build --locked --release -p camera-ffi --target "$target"
  mkdir -p "android/app/src/main/jniLibs/$abi"
  cp "target/$target/release/libcamera_ffi.so" "android/app/src/main/jniLibs/$abi/"
done
if [[ ! -x .tooling/android-bindgen/bin/uniffi-bindgen ]]; then
  cargo install uniffi --version 0.31.2 --features cli --locked --root .tooling/android-bindgen
fi
rm -f android/app/src/main/java/uniffi/camera_ffi/camera_ffi.kt
.tooling/android-bindgen/bin/uniffi-bindgen generate \
  --library target/aarch64-linux-android/release/libcamera_ffi.a \
  --language kotlin --config android/uniffi.toml --out-dir android/app/src/main/java --no-format
test -s android/app/src/main/java/uniffi/camera_ffi/camera_ffi.kt
