import Foundation

actor MockCameraCore: CameraCoreClient {
    func connect(configuration: CameraConnectionConfiguration) async throws -> any CameraSessionClient {
        try await Task.sleep(for: .milliseconds(250))
        return MockCameraSession(stagingRoot: configuration.stagingRoot)
    }
}

actor MockCameraSession: CameraSessionClient {
    private let stagingRoot: URL
    private let assets: [CameraAsset]
    private var disconnected = false

    init(stagingRoot: URL) {
        self.stagingRoot = stagingRoot
        assets = (0..<120).flatMap { index -> [CameraAsset] in
            let group = "mock-group-\(index)"
            let capturedAt = Calendar.current.date(byAdding: .minute, value: -index, to: .now)
            return [
                CameraAsset(
                    id: "mock-jpeg-\(index)",
                    captureGroupID: group,
                    filename: String(format: "DSC_%04d.JPG", index + 1),
                    format: .jpeg,
                    sizeBytes: 2_000_000,
                    capturedAt: capturedAt,
                    width: 6048,
                    height: 4024,
                    thumbnailAvailable: false
                ),
                CameraAsset(
                    id: "mock-nef-\(index)",
                    captureGroupID: group,
                    filename: String(format: "DSC_%04d.NEF", index + 1),
                    format: .nef,
                    sizeBytes: 8_000_000,
                    capturedAt: capturedAt,
                    width: 6048,
                    height: 4024,
                    thumbnailAvailable: false
                ),
            ]
        }
    }

    func info() async throws -> CameraDetails {
        try ensureConnected()
        return CameraDetails(
            id: "mock-z6ii",
            manufacturer: "Nikon",
            model: "Z 6_2",
            displayName: "Nikon Z6 II（模拟测试）",
            firmwareVersion: "C 1.70"
        )
    }

    func capabilities() async throws -> CameraCapabilities {
        try ensureConnected()
        return CameraCapabilities(
            profileID: "nikon-z6ii-c-1.70",
            displayName: "Nikon Z6 II C 1.70",
            supportedFormats: [.jpeg, .nef],
            supportsThumbnails: false,
            supportsPartialDownloads: false,
            supportsEvents: true,
            maxPageSize: 60,
            maxThumbnailBytes: 2_000_000
        )
    }

    func listMedia(
        formats: Set<CameraMediaFormat>,
        pageSize: Int,
        cursor: String?
    ) async throws -> CameraMediaPage {
        try ensureConnected()
        let filtered = formats.isEmpty ? assets : assets.filter { formats.contains($0.format) }
        let offset = cursor.flatMap(Int.init) ?? 0
        let end = min(filtered.count, offset + min(max(pageSize, 1), 60))
        let page = offset < end ? Array(filtered[offset..<end]) : []
        return CameraMediaPage(
            items: page,
            nextCursor: end < filtered.count ? String(end) : nil,
            totalItems: UInt64(filtered.count)
        )
    }

    func loadThumbnail(assetID: String, maxBytes: UInt64) async throws -> Data {
        try ensureConnected()
        guard assets.contains(where: { $0.id == assetID }) else {
            throw CameraBridgeError.protocolFailure("unknown mock asset")
        }
        throw CameraBridgeError.protocolFailure("模拟测试没有真实照片缩略图。")
    }

    func startDownload(asset: CameraAsset, filename: String, overwrite: Bool) async throws -> any CameraTransferClient {
        try ensureConnected()
        let transfer = MockCameraTransfer(
            destination: stagingRoot.appending(path: filename),
            totalBytes: asset.sizeBytes,
            overwrite: overwrite
        )
        await transfer.begin()
        return transfer
    }

    func disconnect() async throws { disconnected = true }

    private func ensureConnected() throws {
        if disconnected { throw CameraBridgeError.sessionInterrupted }
    }
}

actor MockCameraTransfer: CameraTransferClient {
    private let destination: URL
    private let totalBytes: UInt64
    private let overwrite: Bool
    private var progress: CameraTransferProgress
    private var task: Task<Void, Never>?
    private var cancelled = false

    init(destination: URL, totalBytes: UInt64, overwrite: Bool) {
        self.destination = destination
        self.totalBytes = totalBytes
        self.overwrite = overwrite
        progress = CameraTransferProgress(
            phase: .queued,
            bytesTransferred: 0,
            totalBytes: totalBytes,
            destinationURL: nil,
            error: nil
        )
    }

    func begin() {
        guard task == nil else { return }
        task = Task { await run() }
    }

    func snapshot() -> CameraTransferProgress { progress }

    func cancel() { cancelled = true }

    func wait() async -> CameraTransferProgress {
        while !progress.phase.isTerminal {
            try? await Task.sleep(for: .milliseconds(100))
        }
        return progress
    }

    private func run() async {
        progress = CameraTransferProgress(
            phase: .downloading,
            bytesTransferred: 0,
            totalBytes: totalBytes,
            destinationURL: nil,
            error: nil
        )
        for step in 1...10 {
            try? await Task.sleep(for: .milliseconds(80))
            if cancelled {
                progress = CameraTransferProgress(
                    phase: .cancelled,
                    bytesTransferred: progress.bytesTransferred,
                    totalBytes: totalBytes,
                    destinationURL: nil,
                    error: .sessionInterrupted
                )
                return
            }
            progress = CameraTransferProgress(
                phase: .downloading,
                bytesTransferred: totalBytes * UInt64(step) / 10,
                totalBytes: totalBytes,
                destinationURL: nil,
                error: nil
            )
        }

        do {
            try FileManager.default.createDirectory(
                at: destination.deletingLastPathComponent(),
                withIntermediateDirectories: true
            )
            if !overwrite, FileManager.default.fileExists(atPath: destination.path()) {
                throw CocoaError(.fileWriteFileExists)
            }
            try Data(repeating: 0x5A, count: Int(min(totalBytes, 8_000_000))).write(to: destination, options: .atomic)
            progress = CameraTransferProgress(
                phase: .completed,
                bytesTransferred: totalBytes,
                totalBytes: totalBytes,
                destinationURL: destination,
                error: nil
            )
        } catch {
            progress = CameraTransferProgress(
                phase: .failed,
                bytesTransferred: progress.bytesTransferred,
                totalBytes: totalBytes,
                destinationURL: nil,
                error: .other(error.localizedDescription)
            )
        }
    }
}
