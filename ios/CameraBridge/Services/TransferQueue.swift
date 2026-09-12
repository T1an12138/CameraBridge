import Foundation
import Observation
import SwiftData
import UIKit

struct TransferItemState: Identifiable, Equatable {
    let id: UUID
    let asset: CameraAsset
    var progress: CameraTransferProgress
    var localURL: URL?
    var note: String?
}

@MainActor
@Observable
final class TransferQueue {
    private(set) var items: [TransferItemState] = []
    private(set) var isRunning = false

    @ObservationIgnored private let photos: any PhotoImporting
    @ObservationIgnored private var pendingGroups: [PendingGroup] = []
    @ObservationIgnored private var worker: Task<Void, Never>?
    @ObservationIgnored private var activeTransfers: [UUID: any CameraTransferClient] = [:]
    @ObservationIgnored private var cancelledIDs: Set<UUID> = []
    @ObservationIgnored private var records: [UUID: TransferRecord] = [:]
    @ObservationIgnored private weak var modelContext: ModelContext?
    @ObservationIgnored private var hasRestoredRecords = false

    init(photos: any PhotoImporting) {
        self.photos = photos
    }

    /// 启动时恢复导出入口；旧进程中的任务不能假装仍在执行。
    func restore(modelContext: ModelContext) throws {
        guard !hasRestoredRecords else { return }
        let saved = try modelContext.fetch(FetchDescriptor<TransferRecord>())
        self.modelContext = modelContext
        for record in saved {
            let previousPhase = CameraTransferPhase(rawValue: record.statusRawValue) ?? .failed
            if !previousPhase.isTerminal {
                record.statusRawValue = CameraTransferPhase.failed.rawValue
                record.errorMessage = String(localized: "上次传输已中断，请重新连接相机并选择未完成的照片。")
            }
        }
        try modelContext.save()
        for record in saved.sorted(by: { ($0.completedAt ?? .distantPast) > ($1.completedAt ?? .distantPast) }) {
            guard let format = CameraMediaFormat(rawValue: record.formatRawValue) else { continue }
            let phase = CameraTransferPhase(rawValue: record.statusRawValue) ?? .failed
            let localURL = record.localPath.map { URL(filePath: $0) }
                .flatMap { FileManager.default.fileExists(atPath: $0.path()) ? $0 : nil }
            let asset = CameraAsset(
                id: record.cameraAssetID,
                captureGroupID: record.captureGroupID,
                filename: record.filename,
                format: format,
                sizeBytes: UInt64(clamping: record.totalBytes),
                capturedAt: nil,
                width: nil,
                height: nil,
                thumbnailAvailable: false
            )
            records[record.id] = record
            items.append(TransferItemState(
                id: record.id,
                asset: asset,
                progress: CameraTransferProgress(
                    phase: phase,
                    bytesTransferred: UInt64(clamping: record.bytesTransferred),
                    totalBytes: UInt64(clamping: record.totalBytes),
                    destinationURL: localURL,
                    error: phase == .failed ? .sessionInterrupted : nil
                ),
                localURL: localURL,
                note: record.errorMessage ?? (localURL == nil ? nil : String(localized: "原文件已恢复，可继续导出到 Files。"))
            ))
        }
        hasRestoredRecords = true
    }

    func waitUntilIdle() async {
        await worker?.value
    }

    func enqueue(
        assets: [CameraAsset],
        session: any CameraSessionClient,
        modelContext: ModelContext
    ) throws {
        guard !assets.isEmpty else { return }
        try restore(modelContext: modelContext)

        // 不透明媒体 ID 包含相机、存储、对象、大小与拍摄时间，跨重连保持稳定。
        var acceptedIDs = Set(records.values.compactMap { record -> String? in
            let phase = CameraTransferPhase(rawValue: record.statusRawValue) ?? .failed
            return phase == .completed || !phase.isTerminal ? record.cameraAssetID : nil
        })
        let newAssets = assets.filter { acceptedIDs.insert($0.id).inserted }
        guard !newAssets.isEmpty else { return }
        let previousCount = items.count
        let previousGroupCount = pendingGroups.count
        var insertedRecords: [TransferRecord] = []

        let grouped = Dictionary(grouping: newAssets, by: \.captureGroupID)
            .values
            .sorted {
                ($0.first?.capturedAt ?? .distantPast) > ($1.first?.capturedAt ?? .distantPast)
            }

        for group in grouped {
            var itemIDs: [String: UUID] = [:]
            for asset in group.sorted(by: { $0.format.rawValue < $1.format.rawValue }) {
                let id = UUID()
                itemIDs[asset.id] = id
                items.append(
                    TransferItemState(
                        id: id,
                        asset: asset,
                        progress: CameraTransferProgress(
                            phase: .queued,
                            bytesTransferred: 0,
                            totalBytes: asset.sizeBytes,
                            destinationURL: nil,
                            error: nil
                        ),
                        localURL: nil,
                        note: nil
                    )
                )
                let record = TransferRecord(id: id, asset: asset)
                insertedRecords.append(record)
                records[id] = record
                modelContext.insert(record)
            }
            pendingGroups.append(PendingGroup(assets: group, itemIDs: itemIDs, session: session))
        }
        do {
            try modelContext.save()
        } catch {
            for record in insertedRecords {
                records.removeValue(forKey: record.id)
                modelContext.delete(record)
            }
            items.removeLast(items.count - previousCount)
            pendingGroups.removeLast(pendingGroups.count - previousGroupCount)
            throw error
        }
        startWorkerIfNeeded()
    }

    func cancel(_ itemID: UUID) {
        guard let item = items.first(where: { $0.id == itemID }), !item.progress.phase.isTerminal else { return }
        cancelledIDs.insert(itemID)
        if let transfer = activeTransfers[itemID] {
            Task { await transfer.cancel() }
        } else {
            update(itemID) { item in
                item.progress = CameraTransferProgress(
                    phase: .cancelled,
                    bytesTransferred: item.progress.bytesTransferred,
                    totalBytes: item.progress.totalBytes,
                    destinationURL: nil,
                    error: nil
                )
            }
            updateRecord(itemID, with: items.first(where: { $0.id == itemID })?.progress)
        }
    }

    func markExported(_ itemID: UUID) {
        guard let record = records[itemID] else { return }
        record.exportedToFiles = true
        update(itemID) { item in
            item.note = String(localized: "已导出到 Files；满足保留条件后可自动清理本地副本。")
        }
        try? modelContext?.save()
    }

    var canClearFinished: Bool {
        items.contains { canClear($0) }
    }

    func clearFinished() {
        items.removeAll { canClear($0) }
    }

    private func canClear(_ item: TransferItemState) -> Bool {
        guard item.progress.phase == .completed, let record = records[item.id] else { return false }
        return record.photosImported && record.exportedToFiles
    }

    private func startWorkerIfNeeded() {
        guard worker == nil else { return }
        worker = Task { [weak self] in
            await self?.runQueue()
        }
    }

    private func runQueue() async {
        isRunning = true
        let previousIdleTimerDisabled = UIApplication.shared.isIdleTimerDisabled
        UIApplication.shared.isIdleTimerDisabled = true
        defer {
            UIApplication.shared.isIdleTimerDisabled = previousIdleTimerDisabled
            isRunning = false
            worker = nil
        }

        while !pendingGroups.isEmpty {
            let group = pendingGroups.removeFirst()
            var completedFiles: [DownloadedAssetFile] = []

            for asset in group.assets {
                guard let itemID = group.itemIDs[asset.id], !cancelledIDs.contains(itemID) else { continue }
                let filename = stagingFilename(for: asset, itemID: itemID)
                do {
                    let transfer = try await group.session.startDownload(
                        asset: asset,
                        filename: filename,
                        overwrite: false
                    )
                    activeTransfers[itemID] = transfer
                    if cancelledIDs.contains(itemID) {
                        await transfer.cancel()
                        let final = await transfer.wait()
                        activeTransfers[itemID] = nil
                        apply(final, to: itemID)
                        continue
                    }
                    let poller = Task { [weak self] in
                        while !Task.isCancelled {
                            let progress = await transfer.snapshot()
                            guard !Task.isCancelled else { return }
                            self?.apply(progress, to: itemID, persist: false)
                            if progress.phase.isTerminal { return }
                            try? await Task.sleep(for: .milliseconds(250))
                        }
                    }
                    let final = await transfer.wait()
                    poller.cancel()
                    await poller.value
                    activeTransfers[itemID] = nil
                    apply(final, to: itemID)
                    if final.phase == .completed, let url = final.destinationURL {
                        completedFiles.append(DownloadedAssetFile(asset: asset, url: url))
                    }
                } catch {
                    activeTransfers[itemID] = nil
                    apply(
                        CameraTransferProgress(
                            phase: .failed,
                            bytesTransferred: 0,
                            totalBytes: asset.sizeBytes,
                            destinationURL: nil,
                            error: error as? CameraBridgeError ?? .other(error.localizedDescription)
                        ),
                        to: itemID
                    )
                }
            }

            guard !completedFiles.isEmpty else { continue }
            let importResult = await photos.importGroup(completedFiles)
            apply(importResult, to: group, completedFiles: completedFiles)
            try? modelContext?.save()
        }
    }

    private func apply(_ progress: CameraTransferProgress, to id: UUID, persist: Bool = true) {
        update(id) { item in
            item.progress = progress
            item.localURL = progress.destinationURL ?? item.localURL
        }
        if persist {
            updateRecord(id, with: progress)
        }
    }

    private func apply(
        _ result: PhotoImportResult,
        to group: PendingGroup,
        completedFiles: [DownloadedAssetFile]
    ) {
        let preserveUntil = Calendar.current.date(byAdding: .hour, value: 24, to: .now)
        for file in completedFiles {
            guard let id = group.itemIDs[file.asset.id] else { continue }
            let record = records[id]
            record?.preserveUntil = preserveUntil
            switch result {
            case .imported:
                record?.photosImported = true
                update(id) { $0.note = String(localized: "已导入相册，原文件至少保留 24 小时。") }
            case let .jpegOnly(nefURL):
                if file.asset.format == .jpeg {
                    record?.photosImported = true
                    update(id) { $0.note = String(localized: "JPEG 已导入相册。") }
                } else if file.url == nefURL {
                    update(id) { $0.note = String(localized: "系统拒绝该 NEF；原文件已保留，可导出到 Files。") }
                }
            case let .retainedForFiles(_, reason):
                record?.errorMessage = reason.localizedDescription
                update(id) { $0.note = reason.localizedDescription }
            }
        }
    }

    private func update(_ id: UUID, mutation: (inout TransferItemState) -> Void) {
        guard let index = items.firstIndex(where: { $0.id == id }) else { return }
        mutation(&items[index])
    }

    private func updateRecord(_ id: UUID, with progress: CameraTransferProgress?) {
        guard let progress, let record = records[id] else { return }
        record.statusRawValue = progress.phase.rawValue
        record.bytesTransferred = Int64(clamping: progress.bytesTransferred)
        record.totalBytes = Int64(clamping: progress.totalBytes)
        record.localPath = progress.destinationURL?.path()
        record.errorMessage = progress.error?.localizedDescription
        if progress.phase == .completed {
            record.completedAt = .now
            record.preserveUntil = Calendar.current.date(byAdding: .hour, value: 24, to: .now)
        }
        try? modelContext?.save()
    }

    private func stagingFilename(for asset: CameraAsset, itemID: UUID) -> String {
        // 每次重试独占暂存路径；原始名称在 Photos 导入时另行保留。
        let fileExtension = asset.format == .jpeg ? "JPG" : "NEF"
        return "\(itemID.uuidString).\(fileExtension)"
    }
}

private struct PendingGroup {
    let assets: [CameraAsset]
    let itemIDs: [String: UUID]
    let session: any CameraSessionClient
}
