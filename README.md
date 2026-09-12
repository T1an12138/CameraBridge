# CameraBridge

CameraBridge 是一个本地优先的相机导入项目。首个硬件目标是运行 C 固件 1.70 的 Nikon Z6 II，并通过直连 Wi-Fi 接入点模式连接。

本仓库包含：

- 可复用的 Rust PTP/PTP-IP 核心；
- Nikon Z6 II 能力驱动；
- Windows 探测 CLI 和确定性的模拟相机；
- 面向 Apple 平台的窄 UniFFI 边界；
- iOS 26 SwiftUI 应用和远程 macOS 构建流程。

## 当前交付边界

Rust 工作区已通过 Windows 上的 34 项测试。2026-09-12 的 [真实 macOS 构建](https://github.com/T1an12138/CameraBridge/actions/runs/34676925571) 已通过：Xcode 26.6 生成了模拟器 App、Rust XCFramework 与未签名真机归档，12 项 Swift 测试全部成功，并完成模拟器启动截图。实体 iPhone 安装仍需要 Apple 签名与匹配的描述文件；Z6 II 协议兼容性尚未经过实体相机验收。

## Windows 快速开始

```powershell
.\scripts\windows\bootstrap-rust.ps1
.\scripts\windows\cargo.ps1 test --workspace
.\scripts\windows\cargo.ps1 run -p mock-camera '--' --listen 127.0.0.1:15740
```

在第二个终端中运行：

```powershell
.\scripts\windows\cargo.ps1 run -p camera-cli '--' probe --host 127.0.0.1
```

## 没有 Mac 时构建 iOS

将仓库推送到 GitHub 后，`.github/workflows/ios-build.yml` 会在真实 Apple Silicon macOS 26 runner 上生成模拟器 App、Rust XCFramework、无签名真机归档和 Swift 测试结果。也可以在 GitHub 的“Actions → iOS macOS 构建”页面手动运行。

远程构建不需要签名秘密。模拟器 App 默认使用内置 Nikon Z6 II 模拟相机；无签名真机归档只能验证 arm64 编译和链接，不能安装到 iPhone。完整说明见 [docs/macos-ci.md](docs/macos-ci.md)。

有关已验证范围的准确信息，请参阅 [docs/validation-status.md](docs/validation-status.md)；连接实体相机前，请先阅读 [docs/hardware-validation.md](docs/hardware-validation.md)。

## 隐私与协议政策

CameraBridge 不包含账号系统、云端后端或遥测。诊断信息会对凭据脱敏，且绝不包含媒体载荷。本实现使用公开的 PTP/PTP-IP 资料以及对开发者自有硬件的观测结果；不包含 Nikon SDK 二进制文件、反编译的 SnapBridge 代码或 Apple 私有 API。
