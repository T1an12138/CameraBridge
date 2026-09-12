import Foundation
import Testing
@testable import CameraBridge

@MainActor
@Suite("媒体页异步状态")
struct ConnectionPagingTests {
    @Test("快速切换筛选时，只接受最后一次请求的结果")
    func latestFilterWins() async throws {
        let session = DelayedPageSession()
        let coordinator = makeCoordinator(session)
        await coordinator.connect(profile: CameraProfileInput(ssid: "Paging-Mock"), password: "")
        #expect(coordinator.step.isConnected)
        await session.delayNextPages()

        let oldRequest = Task { await coordinator.updateFormats([.jpeg]) }
        try await waitForPendingCount(1, session: session)
        let newRequest = Task { await coordinator.updateFormats([.nef]) }
        try await waitForPendingCount(2, session: session)
        await session.complete(1)
        await newRequest.value
        #expect(coordinator.assets.first?.format == .nef)
        await session.complete(0)
        await oldRequest.value
        #expect(coordinator.assets.first?.format == .nef)
        #expect(coordinator.selectedFormats == [.nef])
        #expect(!coordinator.isLoadingPage)
        await coordinator.disconnect()
    }

    @Test("断开连接后，迟到的媒体页不能恢复旧列表")
    func disconnectInvalidatesPage() async throws {
        let session = DelayedPageSession()
        let coordinator = makeCoordinator(session)
        await coordinator.connect(profile: CameraProfileInput(ssid: "Paging-Mock"), password: "")
        await session.delayNextPages()
        let request = Task { await coordinator.reloadMedia() }
        try await waitForPendingCount(1, session: session)
        await coordinator.disconnect()
        await session.complete(0)
        await request.value
        #expect(coordinator.assets.isEmpty)
        #expect(!coordinator.step.isConnected)
        #expect(!coordinator.isLoadingPage)
    }

    private func makeCoordinator(_ session: DelayedPageSession) -> ConnectionCoordinator {
        ConnectionCoordinator(
            core: PagingCore(session: session), hotspotJoiner: PreviewHotspotJoiner(),
            keychain: KeychainStore(), stagingRoot: FileManager.default.temporaryDirectory
        )
    }

    private func waitForPendingCount(_ count: Int, session: DelayedPageSession) async throws {
        let deadline = ContinuousClock.now.advanced(by: .seconds(5))
        while await session.pendingCount < count {
            guard ContinuousClock.now < deadline else {
                // 即使回归导致请求未发起，也要释放已挂起的 continuation。
                await session.completeAll()
                Issue.record("媒体请求未在限定时间内发起")
                throw CameraBridgeError.sessionInterrupted
            }
            try await Task.sleep(for: .milliseconds(5))
        }
    }
}

private actor PagingCore: CameraCoreClient {
    let session: DelayedPageSession
    init(session: DelayedPageSession) { self.session = session }
    func connect(configuration: CameraConnectionConfiguration) async throws -> any CameraSessionClient { session }
}

private actor DelayedPageSession: CameraSessionClient {
    private let base = MockCameraSession(stagingRoot: FileManager.default.temporaryDirectory)
    private var delayed = false
    private var pending: [(Set<CameraMediaFormat>, CheckedContinuation<CameraMediaPage, any Error>?)] = []
    var pendingCount: Int { pending.count }

    func delayNextPages() { delayed = true }

    func listMedia(formats: Set<CameraMediaFormat>, pageSize: Int, cursor: String?) async throws -> CameraMediaPage {
        if !delayed { return try await base.listMedia(formats: formats, pageSize: pageSize, cursor: cursor) }
        return try await withCheckedThrowingContinuation { continuation in
            pending.append((formats, continuation))
        }
    }

    func complete(_ index: Int) {
        guard pending.indices.contains(index), let continuation = pending[index].1 else { return }
        let format: CameraMediaFormat = pending[index].0.contains(.nef) ? .nef : .jpeg
        pending[index].1 = nil
        continuation.resume(returning: CameraMediaPage(items: [CameraAsset(
            id: format.rawValue, captureGroupID: "group", filename: "photo.\(format.rawValue)",
            format: format, sizeBytes: 3, capturedAt: nil, width: nil, height: nil, thumbnailAvailable: false
        )], nextCursor: nil, totalItems: 1))
    }

    func completeAll() {
        for index in pending.indices { complete(index) }
    }

    func info() async throws -> CameraDetails { try await base.info() }
    func capabilities() async throws -> CameraCapabilities { try await base.capabilities() }
    func loadThumbnail(assetID: String, maxBytes: UInt64) async throws -> Data { throw CameraBridgeError.coreUnavailable }
    func startDownload(asset: CameraAsset, filename: String, overwrite: Bool) async throws -> any CameraTransferClient {
        throw CameraBridgeError.coreUnavailable
    }
    func disconnect() async throws {}
}
