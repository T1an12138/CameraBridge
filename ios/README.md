# CameraBridge iOS

此目录包含原生 iOS 26 应用。生成的 Xcode 工程和 Rust 二进制文件不会提交到版本库中。

## 在本地 Mac 上生成工程

需要 Xcode 26+、Rust 1.98.0 和 XcodeGen：

```bash
bash ./scripts/apple/generate-project.sh
open ios/CameraBridge.xcodeproj
```

脚本会为 arm64 真机和 Apple 芯片模拟器编译 `camera-ffi`，生成 UniFFI 0.31.2 Swift 绑定，创建本地 XCFramework，最后生成 Xcode 工程。安装到 iPhone 前，请选择开发团队，并保持 Hotspot Configuration 能力启用。

## 在 GitHub 的远程 Mac 上构建

`.github/workflows/ios-build.yml` 使用真实的 Apple Silicon `macos-26` runner。工作流会调用：

```bash
bash scripts/apple/ci-build.sh
```

成功后可下载以下产物：

- `CameraBridge-Simulator.app.zip`：Apple Silicon iOS 26 模拟器应用；
- `CameraBridgeCore.xcframework.zip`：arm64 iPhone 与 arm64 模拟器 Rust 核心；
- `CameraBridge-Xcode-Project.zip`：生成后的完整 `ios` 工程目录；
- `CameraBridgeTests.xcresult.zip`：Swift 单元测试和界面测试结果；
- `CameraBridge-Simulator.png`：本轮 Release App 在真实 iOS 模拟器中的启动截图；
- `CameraBridge-Gallery.png`：照片网格截图，模拟器对象没有真实照片预览；
- `CameraBridge-Unsigned.xcarchive.zip`：未签名真机归档，仅用于证明编译和链接；
- `SHA256SUMS` 和三份 `xcodebuild` 日志。

界面使用原生分组列表、蓝色按钮、分段筛选和照片网格，适配深浅色模式。实体 iPhone 的缩略图只通过 Rust/PTP 会话读取对应相机对象，没有离线示例照片。

模拟器无法加入 Nikon 相机热点，因此模拟器版本默认使用内置 Z6 II 模拟相机。启动后会预填模拟 SSID，点击“连接相机”即可浏览 240 个 JPEG/NEF 模拟对象并测试分页、选择和传输状态；模拟对象不提供照片预览。

未签名归档和手工压缩的 `.ipa` 都不能安装到实体 iPhone。可安装版本需要匹配 `app.camerabridge.ios` 的 App ID、启用 Hotspot Configuration 的描述文件，以及相应证书和私钥。TestFlight 还需要 Apple Developer Program 与 App Store Connect 权限。

本应用不申请后台模式或多播 entitlement。当前发现配置使用用户输入的单播主机；只有在 Windows 真机探针记录 Z6 II 的地址行为后，才应调整此配置。
