# 验证状态

状态日期：2026-10-08

## Android 首版验证（2026-10-08）

- 两端界面改为 Apple 风格：分组背景、蓝色按钮、分段筛选和照片缩略图网格，适配深浅色。离线示例照片及资源已移除；实体连接只显示当前相机会话按对象 ID 返回的缩略图，模拟对象明确不提供预览。Android 默认使用实体连接。
- 最终 Android 改版重新通过 3 项 JVM 测试和 5 项 Android 15 模拟器测试（0 跳过），lint 为 0 errors。新增协议界面测试确认不可解码的相机缩略图显示不可用，不使用替代照片。协议模拟对端只返回 JPEG 起止标记，不能作为有效照片解码或实体相机缩略图效果的证据。
- [GitHub Android 构建](https://github.com/T1an12138/CameraBridge/actions/runs/37750632197) 已通过，源码为 `b09ec0e4461f1301b4b4e116bb28bfab881c8d02`；Ubuntu runner 完成两种 ABI 的 Rust 编译、Kotlin 绑定、APK 构建、JVM 测试和 lint，并上传 APK 与校验和。CI 没有运行设备测试，设备测试证据来自本地 Android 15 模拟器。

- Windows 本机构建已生成开发签名 APK，最低 API 29、目标 API 36，包含 arm64-v8a / x86_64 Rust 共享库。
- 34 项 Rust 工作区测试、3 项 Android JVM 测试全部通过。Android lint 为 0 errors；仍有依赖版本等提示，未将提示等同于错误。
- Android 15（API 35）x86_64 模拟器实际安装和启动成功；4 项 instrumentation 测试全部通过、无跳过：演示连接→浏览→下载、100 次真实 UniFFI 对象创建释放及结构化参数错误、通过 `10.0.2.2` 连接主机协议模拟相机进行分页 / 缩略图 / 下载、JPEG 和原始字节文件的 MediaStore 写入与读取校验。
- APK v2 签名已验证；Rust、JNA、AndroidX 原生库的两个 ABI 均具备 16 KB ELF LOAD 对齐，APK 16 KB ZIP 对齐检查通过。
- Android 对硬链接的限制已在真实模拟器测试中暴露并修正：私有 UUID 暂存文件使用原子重命名发布。Kotlin 绑定开启 Android 清理器配置，以支持 API 29。
- 尚未测试实体安卓手机与 Nikon Z6 II、相机热点的实际路由、Android 10 设备运行或实体 NEF 文件解码。

## iOS 界面改版验证（2026-10-08）

- [第 4 次真实 macOS 构建](https://github.com/T1an12138/CameraBridge/actions/runs/37750484328) 已通过，应用源码为 `72fb60173d7759bc0f9b58f9875da2cfaa99b37e`。
- Xcode 26.6 完成模拟器 Release 构建、arm64 未签名真机归档，以及 12 项 Swift 单元测试和 2 项 XCTest 界面测试，全部成功。
- 界面测试验证连接后打开照片网格、选择照片、下载按钮启用、JPEG/NEF 筛选以及模拟对象不显示已加载的照片预览。CI 已生成启动页和网格截图；这不是实体相机缩略图验收。


以下为截至 2026-09-12 的 Windows / macOS 历史验证记录。

## 已在此 Windows 工作区中验证

- Rust 1.98.0 GNU 主机工具链和项目本地的 LLVM-MinGW 包装脚本能够构建完整工作区。
- `cargo test --workspace --all-features` 已通过 34 个单元测试和集成测试，包括目标文件竞争与替换失败时保护原文件的两项回归测试。
- 严格 Clippy 检查（`--workspace --all-targets --all-features -- -D warnings`）和 Rustfmt 检查均已通过。
- `mock-camera` 覆盖了分片/粘包帧、设备忙响应、超时、畸形长度、有界流式传输、中断下载、幂等取消，以及取消后的会话复用。
- UniFFI 0.31.2 能够从 Windows 静态库提取元数据，并生成预期的 Swift API。这只是命名/签名审查，不是 Apple 链接测试。
- 一次 CLI/mock 冒烟测试实现了 3/3 首次尝试成功、稳定枚举 500 个对象、p95 为 134 ms、完成两次下载，并执行了一次重复 SHA-256 比较。这些耗时数据不属于实体相机证据。
- 另一次注入故障的 CLI/mock 运行在 500 个对象下得到首页 p95 145 ms，在首个下载进行到 512 字节后将其中断，随后重连一次、重新开始并完成该文件，且通过了重复 SHA-256 检查。报告模式 3 记录了一次要求的中断恢复和一次已完成的中断恢复。
- 两个 `cargo-fuzz` 解析器目标均可通过 `scripts/windows/check-fuzz.ps1` 完成编译检查；尚未执行长时间模糊测试。
- 2026-09-12 使用当前 Rust 核心执行本地回环压力回归：20/20 次首次连接成功，500 项分页稳定，首页 p95 为 163 ms，50/50 次下载完成，一次人为中断成功恢复，重复下载 SHA-256 一致。报告位于 `target/mock-stress-20260912-1406/validation-report.json`；这不是实体相机证据。

## 已在真实 macOS 环境中验证

- 私有仓库的 [第 3 次构建](https://github.com/T1an12138/CameraBridge/actions/runs/34676925571) 已成功，应用源码提交为 `3809d7c514be2ce79d2e2a55a057ce92f4522399`；后续文档提交不改变这些二进制产物。
- Xcode 26.6、Swift 6 严格并发、iPhoneOS 和 iPhoneSimulator SDK 均已实际用于构建；两平台 Rust 静态库、UniFFI Swift 绑定与 XCFramework 已编译和链接。
- arm64 模拟器 Release App 与未签名 arm64 iPhone 归档已生成；4 个测试套件中的 12 项 Swift 测试（含一个三参数测试）全部通过。
- Release App 已在独立 iOS 26 模拟器上安装、启动并截图。下载到 Windows 的 5 个压缩包及截图均通过 SHA-256 校验，App 的 bundle ID 与平台标识已核对。
- 当前日志保留模拟器固定使用 mock 的不可达分支警告，以及未接入 App Intents 的元数据提取提示；未将它们作为构建失败处理。

## 后续验证

- 运行两个 `cargo-fuzz` 测试任务需要 `nightly` Rust 工具链和检测器运行时。
- 两项媒体请求乱序/断开测试及两项真实 UniFFI 基础测试已在第 3 次远程构建通过：一千次对象创建释放无崩溃，Rust 异步参数错误可正确返回 Swift。
- 一千次对象弱引用释放测试不等于 Instruments 内存泄漏检查；真实 FFI 连接、进度和取消仍需结合相机或网络模拟对端验证。
- 正式 AppIcon 尚未生成，无签名 CI 会暂时禁用 AppIcon 编译。

## 仍需实体硬件验证

自动化工作区运行期间未连接 Nikon Z6 II。以下项目仍待验证，不得报告为已通过：

- Z6 II C 固件 1.70 上的端点/发现行为；
- 20 轮连接/列目录/断开循环以及 19/20 首次尝试成功阈值；
- 获得首个媒体页面的 p95 时间不超过 15 秒；
- 对至少 500 个相机对象进行稳定分页；
- 下载 50 个混合的 JPEG/NEF、三次强制断线以及重复 SHA-256 检查；
- 独立的 USB/读卡器字节比较。

执行该测试时，请遵循 [hardware-validation.md](hardware-validation.md)，且仅保留脱敏后的结果。
