import Foundation
import Testing
@testable import CameraBridge

@Suite("CameraBridge iOS 基础模型")
struct CameraBridgeTests {
    @Test("模拟相机提供确定性分页")
    func mockPaging() async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        let core = MockCameraCore()
        let session = try await core.connect(
            configuration: CameraConnectionConfiguration(
                host: "127.0.0.1",
                port: 15740,
                clientName: "Tests",
                clientID: UUID(),
                connectTimeoutMilliseconds: 1_000,
                requestTimeoutMilliseconds: 1_000,
                stagingRoot: root
            )
        )
        let first = try await session.listMedia(formats: [.jpeg, .nef], pageSize: 60, cursor: nil)
        #expect(first.items.count == 60)
        #expect(first.nextCursor == "60")
        #expect(first.totalItems == 240)
    }

    @Test("传输进度不超过百分之百")
    func progressClamps() {
        let progress = CameraTransferProgress(
            phase: .completed,
            bytesTransferred: 11,
            totalBytes: 10,
            destinationURL: nil,
            error: nil
        )
        #expect(progress.fractionCompleted == 1)
    }

    @Test("完成、取消和失败均为终态", arguments: [
        CameraTransferPhase.completed,
        .cancelled,
        .failed,
    ])
    func terminalPhases(phase: CameraTransferPhase) {
        #expect(phase.isTerminal)
    }
}
