# CameraBridge Android

原生 Kotlin / Jetpack Compose 应用，最低 Android 10（API 29），支持 arm64-v8a 真机与 x86_64 模拟器。通过 UniFFI 0.31.2 / JNA 复用仓库中的 Rust PTP/IP 和 Nikon 驱动。

## 使用

1. 安装 `CameraBridge-Android-0.1.0-debug.apk`。首版是开发签名的测试包，允许系统的“安装未知应用”后可直接安装。
2. 默认使用实体相机连接。界面采用浅灰分组背景、蓝色按钮和照片网格；缩略图由当前相机按对象 ID 返回，读取不到时显示“预览不可用”，不包含离线示例照片。
3. 在相机中启用连接至智能设备，点击“打开 Wi-Fi 设置”，手动加入相机热点；热点无互联网时选择保持连接。
4. 返回应用，输入实际相机 IP，端口默认 15740。`192.168.1.1` 只是预填值，尚未由实体 Z6 II 验证。
5. 下载 JPEG 后可在 `Pictures/CameraBridge` 找到；NEF 保存在 `Download/CameraBridge`。Android 10+ 写入应用自己的媒体不需要申请整个相册权限。

可手动启用演示模式，测试 240 个模拟对象的分页、筛选、选择和传输状态。演示对象没有照片预览，不会导出伪造媒体。

每批最多 100 项，串行下载。切到后台会停止下载；需要重新选择下载，不自动续传。已开始的相册保存会完成后再响应取消。完整文件保存失败时保留在应用私有目录，在传输记录中点击“重试保存”。最近 100 条传输记录以及等待重试保存的文件记录在进程重启后保留，中断任务会显示失败。取消或失败的未完成下载会清理临时文件。首版没有后台服务、自动发现、自动加入热点或 NEF 解码。

## Windows 构建

需要 JDK 17+（设置 `JAVA_HOME`）、Android SDK platform 36 / build-tools 35.0.0、NDK r28b（28.1.13356709），以及仓库自带的 Rust 1.98 Windows GNU 工具链：

```powershell
.\scripts\windows\bootstrap-rust.ps1
.\scripts\android\build.ps1 -SdkRoot 'C:\path\to\Android\Sdk' -NdkRoot 'C:\path\to\android-ndk-r28b'
```

脚本编译两种 ABI、从同轮静态库的元数据生成 Kotlin 绑定、打包共享库和 APK、运行 JVM 测试及 lint，并将 APK / SHA256SUMS 复制到 `target/android-artifacts`。Release `.so` 已去除符号，不用于提取元数据。生成的绑定、原生库和本机 SDK 配置不提交到版本库。原生链接使用 16 KB 最大页面对齐。

## Linux / macOS 构建

```bash
export ANDROID_HOME=/path/to/android-sdk
export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/28.1.13356709"
bash scripts/android/build-native.sh
cd android
chmod +x gradlew
./gradlew :app:assembleDebug :app:testDebugUnitTest :app:lintDebug
```

`.github/workflows/android-build.yml` 提供手动和 main/master 推送构建。产物包含可安装的开发签名 APK 和测试报告，不需要上传签名秘密；该工作流本身是否已执行以实际 Actions 结果为准。

## 设备测试

已连接 Android 设备 / 模拟器时：

```powershell
cd android
.\gradlew.bat :app:connectedDebugAndroidTest
```

测试包含演示连接→照片→下载流程，以及真实 Android JNA/UniFFI 加载与参数错误。`nativeMockRoundtrip` 只在提供 `mockHost` 时连接网络测试对端；没有参数时跳过该网络场景。

要实际运行原生协议连接、分页、缩略图与下载测试，先在仓库根目录启动 `mock-camera`，再转发端口并运行：

```powershell
.\scripts\windows\cargo.ps1 run -p mock-camera '--' --listen 127.0.0.1:15740
# 另一个终端；adb 位于 SDK/platform-tools
adb reverse tcp:15740 tcp:15740
cd android
.\gradlew.bat :app:connectedDebugAndroidTest '-Pandroid.testInstrumentationRunnerArguments.mockHost=127.0.0.1'
```

Android Emulator 也可直接使用主机别名 `10.0.2.2`，无需 `adb reverse`。如果测试服务在受限沙箱中启动，模拟器可能无法与它通信；本次验证在主机环境启动了只监听 127.0.0.1 的模拟相机。

Android 应用不能创建硬链接，因此客户端为私有暂存文件生成随机 UUID，并明确选择 Rust 核心的原子重命名路径；导出到相册 / 下载目录时仍使用相机原始文件名。参见 [Android SELinux 规则](https://android.googlesource.com/platform/system/sepolicy/+/master/private/app_neverallows.te)。

实体相机兼容性、实际热点路由、手机相册 / NEF 文件打开能力仍需结合手机和 Nikon Z6 II 验收。参阅 [硬件验证说明](../docs/hardware-validation.md)。
