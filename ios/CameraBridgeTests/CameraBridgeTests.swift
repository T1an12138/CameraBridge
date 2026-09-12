import Foundation
import Testing
@testable import CameraBridge

@Suite("CameraBridge iOS domain")
struct CameraBridgeTests {
    @Test("Mock camera pages deterministically")
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

    @Test("Transfer progress clamps to one")
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

    @Test("Terminal phases are complete, cancelled, or failed", arguments: [
        CameraTransferPhase.completed,
        .cancelled,
        .failed,
    ])
    func terminalPhases(phase: CameraTransferPhase) {
        #expect(phase.isTerminal)
    }
}
