# 远程 macOS 与 iOS 构建

## 为什么 Windows 不能直接生成可信的 iOS 应用

iOS 编译和链接需要 Xcode 提供的 iPhoneOS、iPhoneSimulator SDK、`xcrun`、`xcodebuild` 和 Apple 签名工具。Windows 容器、Docker 镜像或仅仅模拟 Darwin 命令都不包含这些组件，因而不能替代真正的 macOS 构建机。

CameraBridge 使用 GitHub 托管的 Apple Silicon `macos-26` runner。它是临时的真实 macOS 环境，而不是在 Windows 本机伪造一个 Mac。入口为 `.github/workflows/ios-build.yml`，实际构建逻辑位于 `scripts/apple/ci-build.sh`。

## 触发方式

仓库推送到 GitHub 的 `master` 或 `main` 分支后，只要 Rust、iOS、Apple 脚本或工作流发生变化，构建就会自动触发。也可以在 GitHub 仓库中打开“Actions → iOS macOS 构建 → Run workflow”手动触发。

工作流只申请仓库内容读取权限，不读取签名证书、Apple 账号或相机凭据。首次运行需要安装 XcodeGen，并通过 Cargo 下载 Rust 依赖；后续运行会复用缓存。

## 构建过程

1. 选择 Xcode 26.6，并验证 iPhoneOS 与 iPhoneSimulator SDK。
2. 固定 Rust 1.98.0，编译 `aarch64-apple-ios` 和 `aarch64-apple-ios-sim`。
3. 使用 UniFFI 0.31.2 生成 Swift 绑定，并创建 XCFramework。
4. 使用 XcodeGen 生成 `CameraBridge.xcodeproj`。
5. 构建 arm64 模拟器 App。
6. 生成无签名 arm64 真机 `.xcarchive`。
7. 在 iPhone 17 Pro iOS 26 模拟器上运行 Swift 测试。
8. 校验 App 的 `Info.plist` 和可执行文件格式，打包产物并生成 SHA-256。
9. 测试成功后创建独立模拟器，安装并启动 Release App，保存 `CameraBridge-Simulator.png` 和启动日志，然后删除本次创建的模拟器。

如果 Swift 测试失败，工作流仍会保留已经生成的应用、测试结果和日志，然后以失败状态结束。

## 产物用途

| 产物 | 用途 | 能否装到实体 iPhone |
| --- | --- | --- |
| `CameraBridge-Simulator.app.zip` | 在 Apple Silicon iOS 26 模拟器中运行 UI 和 mock 流程 | 否 |
| `CameraBridgeCore.xcframework.zip` | 将 Rust 核心集成到 Apple 工程 | 不能单独安装 |
| `CameraBridge-Xcode-Project.zip` | 在 Xcode 中继续开发和签名 | 需重新构建签名 |
| `CameraBridgeTests.xcresult.zip` | 在 Xcode 中查看测试结果 | 不适用 |
| `CameraBridge-Unsigned.xcarchive.zip` | 验证真机 arm64 编译与链接 | 否 |

不要把未签名 App 手工放进 `Payload` 后改名为 `.ipa`。这种文件虽然扩展名正确，但缺少有效代码签名和描述文件，iOS 会拒绝安装。

## 在 Mac 模拟器中安装

解压模拟器 App，启动一个兼容的 iOS 26 模拟器，然后运行：

```bash
xcrun simctl install booted /绝对路径/CameraBridge.app
xcrun simctl launch booted app.camerabridge.ios
```

模拟器版本会自动使用内置 Nikon Z6 II mock，并显示“模拟器演示”提示。模拟器不能测试 `NEHotspotConfigurationManager` 加入真实相机热点，也不能替代实体 iPhone 的本地网络权限测试。

## 生成可安装真机版本

还需要以下外部条件：

- Apple Developer Program 会员；
- `app.camerabridge.ios` 对应的 App ID；
- 启用 `com.apple.developer.networking.HotspotConfiguration` 的描述文件；
- Apple Development 或 Apple Distribution 证书及其私钥；
- Development/Ad Hoc 分发时登记目标 iPhone 的 UDID；
- TestFlight 发布时具备 App Store Connect 权限和正式 App 图标。

证书私钥和描述文件不得提交到仓库。后续接入签名 CI 时应使用 GitHub Environments 和受保护的加密 secrets。

## 当前限制

- 当前 XCFramework 的模拟器切片仅包含 arm64，因此必须使用 Apple Silicon runner 或 Mac。
- 模拟器使用 mock 相机，不能证明 Z6 II 的真实 Wi-Fi/PTP/IP 兼容性。
- AppIcon 资源尚未包含正式的 1024×1024 图片；无签名 CI 临时禁用 AppIcon 编译，TestFlight 前必须补齐。
- 在第一次远程工作流成功完成前，iOS 构建状态仍应记录为“尚未执行”。
