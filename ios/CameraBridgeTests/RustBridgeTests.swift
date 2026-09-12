import CameraBridgeCore
import Foundation
import Testing

@Suite("真实 Rust 桥接基础验证")
struct RustBridgeTests {
    @Test("一千次创建与释放 UniFFI 核心对象")
    func coreLifetimes() {
        for _ in 0..<1_000 {
            weak var releasedCore: CameraBridgeCore.CameraCore?
            autoreleasepool {
                let core = CameraBridgeCore.CameraCore()
                releasedCore = core
            }
            #expect(releasedCore == nil)
        }
    }

    @Test("Rust 异步参数错误通过 UniFFI 返回 Swift")
    func asynchronousErrorRoundTrip() async {
        let core = CameraBridgeCore.CameraCore()
        do {
            // 空暂存路径在 Rust 校验层拒绝，不访问网络或真实相机。
            _ = try await core.connect(config: FfiConnectionConfig(
                host: "127.0.0.1",
                port: 15740,
                clientName: "CameraBridge Tests",
                clientId: UUID().uuidString,
                connectTimeoutMs: 100,
                requestTimeoutMs: 100,
                stagingRoot: ""
            ))
            Issue.record("无效暂存路径应由 Rust 拒绝")
        } catch CameraFfiError.Failure(let code, let message) {
            #expect(code == .invalidInput)
            #expect(!message.isEmpty)
        } catch {
            Issue.record("收到非预期桥接错误：\(error)")
        }
    }
}
