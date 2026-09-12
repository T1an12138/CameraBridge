import Foundation
import SwiftData
import Testing
@testable import CameraBridge

@MainActor
@Suite("传输队列持久化与去重")
struct TransferQueueTests {
    @Test("同一批、同一次运行和重启后都不重复下载已完成对象")
    func repeatedSelection() async throws {
        let container = try makeContainer()
        let context = container.mainContext
        let queue = TransferQueue(photos: RetainingPhotos())
        let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        let session = RecordingSession(root: root)
        let asset = makeAsset()

        try queue.enqueue(assets: [asset, asset], session: session, modelContext: context)
        try queue.enqueue(assets: [asset], session: session, modelContext: context)
        await queue.waitUntilIdle()
        #expect(queue.items.count == 1)
        #expect(queue.items.first?.progress.phase == .completed)
        #expect(await session.filenames.count == 1)

        let restored = TransferQueue(photos: RetainingPhotos())
        try restored.restore(modelContext: context)
        try restored.enqueue(assets: [asset], session: session, modelContext: context)
        await restored.waitUntilIdle()
        #expect(await session.filenames.count == 1)
        #expect(try context.fetchCount(FetchDescriptor<TransferRecord>()) == 1)
        #expect(restored.items.first?.localURL != nil)
    }

    @Test("重启恢复原文件导出入口，重复恢复不会增加行")
    func restoresExport() throws {
        let container = try makeContainer()
        let context = container.mainContext
        let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let url = root.appending(path: "retained.NEF")
        try Data([1, 2, 3]).write(to: url)
        let record = TransferRecord(asset: makeAsset())
        record.statusRawValue = CameraTransferPhase.completed.rawValue
        record.localPath = url.path()
        record.completedAt = .now
        record.bytesTransferred = 3
        context.insert(record)
        try context.save()

        let queue = TransferQueue(photos: RetainingPhotos())
        try queue.restore(modelContext: context)
        try queue.restore(modelContext: context)
        #expect(queue.items.count == 1)
        #expect(queue.items.first?.localURL == url)
        queue.clearFinished()
        #expect(queue.items.count == 1)
        #expect(FileManager.default.fileExists(atPath: url.path()))
    }

    @Test("重启后的未完成任务标记中断，已完成状态保持不变")
    func restoresInterrupted() throws {
        let container = try makeContainer()
        let context = container.mainContext
        let interrupted = TransferRecord(asset: makeAsset())
        interrupted.statusRawValue = CameraTransferPhase.downloading.rawValue
        interrupted.bytesTransferred = 1
        context.insert(interrupted)
        try context.save()
        let queue = TransferQueue(photos: RetainingPhotos())
        try queue.restore(modelContext: context)
        #expect(queue.items.first?.progress.phase == .failed)
        #expect(interrupted.statusRawValue == CameraTransferPhase.failed.rawValue)
        #expect(interrupted.errorMessage != nil)
    }

    @Test("重试失败对象使用新路径，取消已完成任务不回退状态")
    func retryAndLateCancel() async throws {
        let container = try makeContainer()
        let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        let session = RecordingSession(root: root, failFirst: true)
        let queue = TransferQueue(photos: RetainingPhotos())
        let asset = makeAsset()
        try queue.enqueue(assets: [asset], session: session, modelContext: container.mainContext)
        await queue.waitUntilIdle()
        #expect(queue.items.first?.progress.phase == .failed)
        try queue.enqueue(assets: [asset], session: session, modelContext: container.mainContext)
        await queue.waitUntilIdle()
        let names = await session.filenames
        #expect(names.count == 2)
        #expect(Set(names).count == 2)
        let completed = try #require(queue.items.first(where: { $0.progress.phase == .completed }))
        queue.cancel(completed.id)
        #expect(queue.items.first(where: { $0.id == completed.id })?.progress.phase == .completed)
    }

    @Test("保留期结束也不删除尚未导出或导入失败的原图")
    func retentionRequiresBothSuccesses() throws {
        let container = try makeContainer()
        let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        var records: [TransferRecord] = []
        for index in 0..<3 {
            let url = root.appending(path: "\(index).NEF")
            try Data([1]).write(to: url)
            let record = TransferRecord(asset: makeAsset())
            record.statusRawValue = CameraTransferPhase.completed.rawValue
            record.localPath = url.path()
            record.preserveUntil = .distantPast
            record.photosImported = index != 1
            record.exportedToFiles = index != 0
            container.mainContext.insert(record)
            records.append(record)
        }
        try container.mainContext.save()
        StagingRetentionService().removeEligibleFiles(records: records, modelContext: container.mainContext)
        #expect(FileManager.default.fileExists(atPath: root.appending(path: "0.NEF").path()))
        #expect(FileManager.default.fileExists(atPath: root.appending(path: "1.NEF").path()))
        #expect(!FileManager.default.fileExists(atPath: root.appending(path: "2.NEF").path()))
    }

    private func makeContainer() throws -> ModelContainer {
        try ModelContainer(for: TransferRecord.self, configurations: ModelConfiguration(isStoredInMemoryOnly: true))
    }

    private func makeAsset() -> CameraAsset {
        CameraAsset(id: "opaque-test-object", captureGroupID: "group-1", filename: "DSC_0001.NEF",
                    format: .nef, sizeBytes: 3, capturedAt: nil, width: nil, height: nil, thumbnailAvailable: false)
    }
}

private actor RetainingPhotos: PhotoImporting {
    func importGroup(_ files: [DownloadedAssetFile]) async -> PhotoImportResult {
        .retainedForFiles(files.map(\.url), reason: .photosDenied)
    }
}

private actor RecordingSession: CameraSessionClient {
    let root: URL
    let failFirst: Bool
    private(set) var filenames: [String] = []

    init(root: URL, failFirst: Bool = false) {
        self.root = root
        self.failFirst = failFirst
    }

    func startDownload(asset: CameraAsset, filename: String, overwrite: Bool) async throws -> any CameraTransferClient {
        filenames.append(filename)
        if failFirst && filenames.count == 1 {
            throw CameraBridgeError.sessionInterrupted
        }
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        let url = root.appending(path: filename)
        try Data([1, 2, 3]).write(to: url)
        return FinishedTransfer(progress: CameraTransferProgress(
            phase: .completed, bytesTransferred: 3, totalBytes: 3, destinationURL: url, error: nil
        ))
    }

    func info() async throws -> CameraDetails { throw CameraBridgeError.coreUnavailable }
    func capabilities() async throws -> CameraCapabilities { throw CameraBridgeError.coreUnavailable }
    func listMedia(formats: Set<CameraMediaFormat>, pageSize: Int, cursor: String?) async throws -> CameraMediaPage {
        throw CameraBridgeError.coreUnavailable
    }
    func loadThumbnail(assetID: String, maxBytes: UInt64) async throws -> Data { throw CameraBridgeError.coreUnavailable }
    func disconnect() async throws {}
}

private actor FinishedTransfer: CameraTransferClient {
    let progress: CameraTransferProgress
    init(progress: CameraTransferProgress) { self.progress = progress }
    func snapshot() async -> CameraTransferProgress { progress }
    func wait() async -> CameraTransferProgress { progress }
    func cancel() async {}
}
