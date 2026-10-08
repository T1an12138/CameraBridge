#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd -P)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd -P)"
IOS_ROOT="$PROJECT_ROOT/ios"
TARGET_ROOT="$PROJECT_ROOT/target"
OUTPUT_ROOT="${CAMERABRIDGE_OUTPUT_DIR:-$TARGET_ROOT/apple-ci-artifacts}"
DERIVED_DATA="${CAMERABRIDGE_DERIVED_DATA:-$TARGET_ROOT/apple-ci-derived-data}"
WORK_ROOT="${CAMERABRIDGE_WORK_DIR:-$TARGET_ROOT/apple-ci-work}"
TEST_DESTINATION="${CAMERABRIDGE_TEST_DESTINATION:-platform=iOS Simulator,name=iPhone 17 Pro,OS=latest}"
PROJECT_FILE="$IOS_ROOT/CameraBridge.xcodeproj"
SCHEME="CameraBridge"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "此脚本必须在真实 macOS/Xcode 环境中运行。" >&2
  exit 1
fi

for tool in cargo rustup xcodebuild xcodegen xcrun jq ditto file plutil shasum; do
  command -v "$tool" >/dev/null || {
    echo "缺少构建工具：$tool" >&2
    exit 1
  }
done

mkdir -p "$TARGET_ROOT" "$OUTPUT_ROOT" "$DERIVED_DATA" "$WORK_ROOT"
TARGET_ROOT="$(cd "$TARGET_ROOT" && pwd -P)"
OUTPUT_ROOT="$(cd "$OUTPUT_ROOT" && pwd -P)"
DERIVED_DATA="$(cd "$DERIVED_DATA" && pwd -P)"
WORK_ROOT="$(cd "$WORK_ROOT" && pwd -P)"

for directory in "$OUTPUT_ROOT" "$DERIVED_DATA" "$WORK_ROOT"; do
  case "$directory/" in
    "$TARGET_ROOT/"*) ;;
    *)
      echo "拒绝使用项目 target 目录之外的构建路径：$directory" >&2
      exit 1
      ;;
  esac
done

ARCHIVE_PATH="$WORK_ROOT/CameraBridge-unsigned.xcarchive"
TEST_RESULT="$WORK_ROOT/CameraBridgeTests.xcresult"

rm -f \
  "$OUTPUT_ROOT/CameraBridge-Simulator.app.zip" \
  "$OUTPUT_ROOT/CameraBridge-Unsigned.xcarchive.zip" \
  "$OUTPUT_ROOT/CameraBridgeCore.xcframework.zip" \
  "$OUTPUT_ROOT/CameraBridge-Xcode-Project.zip" \
  "$OUTPUT_ROOT/CameraBridgeTests.xcresult.zip" \
  "$OUTPUT_ROOT/CameraBridge-Simulator.png" \
  "$OUTPUT_ROOT/CameraBridge-Gallery.png" \
  "$OUTPUT_ROOT/simulator-launch.log" \
  "$OUTPUT_ROOT/SHA256SUMS" \
  "$OUTPUT_ROOT/artifact-validation.log" \
  "$OUTPUT_ROOT/xcodebuild-simulator.log" \
  "$OUTPUT_ROOT/xcodebuild-device-unsigned.log" \
  "$OUTPUT_ROOT/xcodebuild-tests.log"
rm -rf "$ARCHIVE_PATH" "$TEST_RESULT"

bash "$SCRIPT_DIR/generate-project.sh"

COMMON_BUILD_ARGUMENTS=(
  -project "$PROJECT_FILE"
  -scheme "$SCHEME"
  -configuration Release
  -derivedDataPath "$DERIVED_DATA"
)

xcodebuild "${COMMON_BUILD_ARGUMENTS[@]}" \
  -sdk iphonesimulator \
  -destination "generic/platform=iOS Simulator" \
  ARCHS=arm64 \
  ONLY_ACTIVE_ARCH=YES \
  CODE_SIGNING_ALLOWED=NO \
  CODE_SIGNING_REQUIRED=NO \
  ASSETCATALOG_COMPILER_APPICON_NAME= \
  COMPILER_INDEX_STORE_ENABLE=NO \
  clean build 2>&1 | tee "$OUTPUT_ROOT/xcodebuild-simulator.log"

xcodebuild \
  -project "$PROJECT_FILE" \
  -scheme "$SCHEME" \
  -configuration Release \
  -destination "generic/platform=iOS" \
  -archivePath "$ARCHIVE_PATH" \
  ARCHS=arm64 \
  CODE_SIGNING_ALLOWED=NO \
  CODE_SIGNING_REQUIRED=NO \
  CODE_SIGN_IDENTITY= \
  ASSETCATALOG_COMPILER_APPICON_NAME= \
  COMPILER_INDEX_STORE_ENABLE=NO \
  archive 2>&1 | tee "$OUTPUT_ROOT/xcodebuild-device-unsigned.log"

SIMULATOR_APP="$DERIVED_DATA/Build/Products/Release-iphonesimulator/CameraBridge.app"
DEVICE_APP="$ARCHIVE_PATH/Products/Applications/CameraBridge.app"
XCFRAMEWORK="$IOS_ROOT/Packages/CameraBridgeCorePackage/Artifacts/camera_ffiFFI.xcframework"

test -d "$SIMULATOR_APP"
test -x "$SIMULATOR_APP/CameraBridge"
test -d "$DEVICE_APP"
test -x "$DEVICE_APP/CameraBridge"
test -d "$XCFRAMEWORK"
test -d "$PROJECT_FILE"

{
  echo "模拟器可执行文件："
  file "$SIMULATOR_APP/CameraBridge"
  plutil -lint "$SIMULATOR_APP/Info.plist"
  echo "未签名真机可执行文件："
  file "$DEVICE_APP/CameraBridge"
  plutil -lint "$DEVICE_APP/Info.plist"
} | tee "$OUTPUT_ROOT/artifact-validation.log"

ditto -c -k --sequesterRsrc --keepParent \
  "$SIMULATOR_APP" "$OUTPUT_ROOT/CameraBridge-Simulator.app.zip"
ditto -c -k --sequesterRsrc --keepParent \
  "$ARCHIVE_PATH" "$OUTPUT_ROOT/CameraBridge-Unsigned.xcarchive.zip"
ditto -c -k --sequesterRsrc --keepParent \
  "$XCFRAMEWORK" "$OUTPUT_ROOT/CameraBridgeCore.xcframework.zip"
ditto -c -k --sequesterRsrc --keepParent \
  "$IOS_ROOT" "$OUTPUT_ROOT/CameraBridge-Xcode-Project.zip"

set +e
xcodebuild \
  -project "$PROJECT_FILE" \
  -scheme "$SCHEME" \
  -configuration Debug \
  -derivedDataPath "$DERIVED_DATA-tests" \
  -destination "$TEST_DESTINATION" \
  -resultBundlePath "$TEST_RESULT" \
  ARCHS=arm64 \
  ONLY_ACTIVE_ARCH=YES \
  CODE_SIGNING_ALLOWED=NO \
  CODE_SIGNING_REQUIRED=NO \
  ASSETCATALOG_COMPILER_APPICON_NAME= \
  COMPILER_INDEX_STORE_ENABLE=NO \
  test 2>&1 | tee "$OUTPUT_ROOT/xcodebuild-tests.log"
TEST_STATUS=${PIPESTATUS[0]}
set -e

if [[ -d "$TEST_RESULT" ]]; then
  ditto -c -k --sequesterRsrc --keepParent \
    "$TEST_RESULT" "$OUTPUT_ROOT/CameraBridgeTests.xcresult.zip"
fi

ZIP_FILES=(
  CameraBridge-Simulator.app.zip
  CameraBridge-Unsigned.xcarchive.zip
  CameraBridgeCore.xcframework.zip
  CameraBridge-Xcode-Project.zip
)
if [[ -f "$OUTPUT_ROOT/CameraBridgeTests.xcresult.zip" ]]; then
  ZIP_FILES+=(CameraBridgeTests.xcresult.zip)
fi

(
  cd "$OUTPUT_ROOT"
  shasum -a 256 "${ZIP_FILES[@]}" > SHA256SUMS
)

if [[ "$TEST_STATUS" -ne 0 ]]; then
  echo "Swift 测试失败，测试结果和构建日志已保留。" >&2
  exit "$TEST_STATUS"
fi

# 使用本次运行独占的模拟器验证 App 能启动，并保留真实界面截图。
RUNTIME_ID="$(xcrun simctl list runtimes --json | jq -r \
  '[.runtimes[] | select(.isAvailable and (.identifier | contains("SimRuntime.iOS-26-")))] | sort_by(.version | split(".") | map(tonumber)) | last | .identifier // empty')"
if [[ -z "$RUNTIME_ID" ]]; then
  echo "找不到可用的 iOS 26 模拟器运行时。" >&2
  exit 1
fi
SMOKE_DEVICE="$(xcrun simctl create 'CameraBridge CI' 'iPhone 17 Pro' "$RUNTIME_ID")"
trap 'xcrun simctl shutdown "$SMOKE_DEVICE" >/dev/null 2>&1 || true; xcrun simctl delete "$SMOKE_DEVICE" >/dev/null 2>&1 || true' EXIT
xcrun simctl boot "$SMOKE_DEVICE"
xcrun simctl bootstatus "$SMOKE_DEVICE" -b
xcrun simctl install "$SMOKE_DEVICE" "$SIMULATOR_APP"
xcrun simctl launch "$SMOKE_DEVICE" app.camerabridge.ios | tee "$OUTPUT_ROOT/simulator-launch.log"
sleep 3
xcrun simctl io "$SMOKE_DEVICE" screenshot "$OUTPUT_ROOT/CameraBridge-Simulator.png"
xcrun simctl terminate "$SMOKE_DEVICE" app.camerabridge.ios
SIMCTL_CHILD_CAMERABRIDGE_PREVIEW_PHOTOS=1 xcrun simctl launch "$SMOKE_DEVICE" app.camerabridge.ios
sleep 3
xcrun simctl io "$SMOKE_DEVICE" screenshot "$OUTPUT_ROOT/CameraBridge-Gallery.png"
(
  cd "$OUTPUT_ROOT"
  shasum -a 256 CameraBridge-Simulator.png >> SHA256SUMS
  shasum -a 256 CameraBridge-Gallery.png >> SHA256SUMS
)

echo "iOS 构建、归档和测试产物已写入：$OUTPUT_ROOT"
