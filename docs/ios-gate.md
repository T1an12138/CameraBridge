# iOS 构建与发布门槛

项目有意不将 iOS 源码描述为已在 Windows 上完成构建验证。

完成 iOS 里程碑前，需要具备：

- 一台能够运行 Xcode 26 或更高版本的 Mac；
- 生成的 XCFramework 所支持的 Apple silicon 或 Intel 模拟器目标；
- 一台运行 iOS 26 的实体 iPhone；
- Nikon Z6 II 热点访问条件；
- 在通过 TestFlight 分发前开通 Apple Developer Program 会员资格。

SwiftUI 源码脚手架可以在 Windows 上维护，但尚未经过构建验证，也未达到生产就绪状态。在 macOS 上，应先验证 FFI 边界：构建真机与模拟器静态库、创建 XCFramework、以启用严格并发的 Swift 6 语言模式编译、验证进度与取消，并在 Instruments 下重复执行 1,000 次对象创建/释放。如果未通过此门槛，不得将 UI 视为可发布状态。
