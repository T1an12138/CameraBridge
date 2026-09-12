# 验证状态

状态日期：2026-09-12

## 已在此 Windows 工作区中验证

- Rust 1.98.0 GNU 主机工具链和项目本地的 LLVM-MinGW 包装脚本能够构建完整工作区。
- `cargo test --workspace --all-features` 已通过 34 个单元测试和集成测试，包括目标文件竞争与替换失败时保护原文件的两项回归测试。
- 严格 Clippy 检查（`--workspace --all-targets --all-features -- -D warnings`）和 Rustfmt 检查均已通过。
- `mock-camera` 覆盖了分片/粘包帧、设备忙响应、超时、畸形长度、有界流式传输、中断下载、幂等取消，以及取消后的会话复用。
- UniFFI 0.31.2 能够从 Windows 静态库提取元数据，并生成预期的 Swift API。这只是命名/签名审查，不是 Apple 链接测试。
- 一次 CLI/mock 冒烟测试实现了 3/3 首次尝试成功、稳定枚举 500 个对象、p95 为 134 ms、完成两次下载，并执行了一次重复 SHA-256 比较。这些耗时数据不属于实体相机证据。
- 另一次注入故障的 CLI/mock 运行在 500 个对象下得到首页 p95 145 ms，在首个下载进行到 512 字节后将其中断，随后重连一次、重新开始并完成该文件，且通过了重复 SHA-256 检查。报告模式 3 记录了一次要求的中断恢复和一次已完成的中断恢复。
- 两个 `cargo-fuzz` 解析器目标均可通过 `scripts/windows/check-fuzz.ps1` 完成编译检查；尚未执行长时间模糊测试。

## 已实现但尚未执行

- 运行两个 `cargo-fuzz` 测试任务需要 `nightly` Rust 工具链和检测器运行时。
- 已连接私有仓库 `T1an12138/CameraBridge` 并启动真实 GitHub Actions `macos-26` 构建。Xcode 26.6、iPhoneOS 和 iPhoneSimulator SDK 检查已通过；应用编译及测试结果仍待首轮运行结束确认。
- 新增五项 Swift 传输回归测试，覆盖重复选择、重启恢复导出、未完成任务中断、重试路径与取消，以及原文件清理条件。
- iOS 模拟器会注入内置 Z6 II mock 与模拟热点加入器；该路径仍需在真实 iOS 26 模拟器中构建和运行验证。
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
