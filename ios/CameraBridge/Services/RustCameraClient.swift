import Foundation

#if canImport(CameraBridgeCore)
import CameraBridgeCore

actor RustCameraCore: CameraCoreClient {
    private let core = CameraBridgeCore.CameraCore()

    func connect(configuration: CameraConnectionConfiguration) async throws -> any CameraSessionClient {
        do {
            let session = try await core.connect(
                config: FfiConnectionConfig(
                    host: configuration.host,
                    port: configuration.port,
                    clientName: configuration.clientName,
                    clientId: configuration.clientID.uuidString,
                    connectTimeoutMs: configuration.connectTimeoutMilliseconds,
                    requestTimeoutMs: configuration.requestTimeoutMilliseconds,
                    stagingRoot: configuration.stagingRoot.path()
                )
            )
            return RustCameraSession(session: session)
        } catch {
            throw mapFFIError(error)
        }
    }
}

actor RustCameraSession: CameraSessionClient {
    private let session: FfiCameraSession

    init(session: FfiCameraSession) {
        self.session = session
    }

    func info() async throws -> CameraDetails {
        do {
            let value = try await session.info()
            return CameraDetails(
                id: value.id,
                manufacturer: value.manufacturer,
                model: value.model,
                displayName: value.displayName,
                firmwareVersion: value.firmwareVersion
            )
        } catch {
            throw mapFFIError(error)
        }
    }

    func capabilities() async throws -> CameraCapabilities {
        do {
            let value = try await session.capabilities()
            return CameraCapabilities(
                profileID: value.profileId,
                displayName: value.displayName,
                supportedFormats: Set(value.supportedFormats.map(\.cameraFormat)),
                supportsThumbnails: value.supportsThumbnails,
                supportsPartialDownloads: value.supportsPartialDownloads,
                supportsEvents: value.supportsEvents,
                maxPageSize: Int(value.maxPageSize),
                maxThumbnailBytes: value.maxThumbnailBytes
            )
        } catch {
            throw mapFFIError(error)
        }
    }

    func listMedia(
        formats: Set<CameraMediaFormat>,
        pageSize: Int,
        cursor: String?
    ) async throws -> CameraMediaPage {
        do {
            let value = try await session.listMedia(
                formats: formats.map(\.ffiFormat),
                pageSize: UInt32(clamping: pageSize),
                cursor: cursor
            )
            return CameraMediaPage(
                items: value.items.map(\.cameraAsset),
                nextCursor: value.nextCursor,
                totalItems: value.totalItems
            )
        } catch {
            throw mapFFIError(error)
        }
    }

    func loadThumbnail(assetID: String, maxBytes: UInt64) async throws -> Data {
        do {
            return try await session.loadThumbnail(assetId: assetID, maxBytes: maxBytes).data
        } catch {
            throw mapFFIError(error)
        }
    }

    func startDownload(asset: CameraAsset, filename: String, overwrite: Bool) async throws -> any CameraTransferClient {
        do {
            let job = try await session.startDownload(
                assetId: asset.id,
                filename: filename,
                expectedSize: asset.sizeBytes,
                overwrite: overwrite
            )
            return RustCameraTransfer(job: job)
        } catch {
            throw mapFFIError(error)
        }
    }

    func disconnect() async throws {
        do {
            try await session.disconnect()
        } catch {
            throw mapFFIError(error)
        }
    }
}

actor RustCameraTransfer: CameraTransferClient {
    private let job: FfiTransferJob

    init(job: FfiTransferJob) {
        self.job = job
    }

    func snapshot() async -> CameraTransferProgress { job.snapshot().cameraProgress }
    func cancel() async { job.cancel() }
    func wait() async -> CameraTransferProgress { await job.wait().cameraProgress }
}

private extension FfiMediaFormat {
    var cameraFormat: CameraMediaFormat {
        switch self {
        case .jpeg: .jpeg
        case .nef: .nef
        }
    }
}

private extension CameraMediaFormat {
    var ffiFormat: FfiMediaFormat {
        switch self {
        case .jpeg: .jpeg
        case .nef: .nef
        }
    }
}

private extension FfiCameraAsset {
    var cameraAsset: CameraAsset {
        CameraAsset(
            id: id,
            captureGroupID: captureGroupId,
            filename: filename,
            format: format.cameraFormat,
            sizeBytes: sizeBytes,
            capturedAt: Self.captureDate(from: capturedAt),
            width: width.map(Int.init),
            height: height.map(Int.init),
            thumbnailAvailable: thumbnailAvailable
        )
    }

    static func captureDate(from value: String?) -> Date? {
        guard let value else { return nil }

        let formatter = DateFormatter()
        formatter.calendar = Calendar(identifier: .gregorian)
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.dateFormat = "yyyy-MM-dd'T'HH:mm:ss"
        return formatter.date(from: value)
    }
}

private extension FfiTransferSnapshot {
    var cameraProgress: CameraTransferProgress {
        CameraTransferProgress(
            phase: phase.cameraPhase,
            bytesTransferred: bytesTransferred,
            totalBytes: totalBytes,
            destinationURL: destinationPath.map { URL(filePath: $0) },
            error: errorCode.map { mapFFIErrorCode($0, message: errorMessage ?? "") }
        )
    }
}

private extension FfiTransferPhase {
    var cameraPhase: CameraTransferPhase {
        switch self {
        case .queued: .queued
        case .downloading: .downloading
        case .finalizing: .finalizing
        case .completed: .completed
        case .cancelled: .cancelled
        case .failed: .failed
        }
    }
}

private func mapFFIError(_ error: Error) -> CameraBridgeError {
    if let ffiError = error as? CameraFfiError,
       case let .Failure(code: code, reason: message) = ffiError {
        return mapFFIErrorCode(code, message: message)
    }
    return .other(error.localizedDescription)
}

private func mapFFIErrorCode(_ code: FfiErrorCode, message: String) -> CameraBridgeError {
    switch code {
    case .cameraBusy: .cameraBusy
    case .cameraAsleep: .cameraAsleep
    case .unsupportedFirmware, .unsupportedOperation: .unsupportedFirmware
    case .permissionDenied: .localNetworkDenied
    case .insufficientSpace: .insufficientSpace
    case .sessionInterrupted, .timeout, .connectionFailed: .sessionInterrupted
    case .protocol: .protocolFailure(message)
    case .invalidInput: .invalidConfiguration(message)
    default: .other(message)
    }
}

#else

actor RustCameraCore: CameraCoreClient {
    func connect(configuration: CameraConnectionConfiguration) async throws -> any CameraSessionClient {
        throw CameraBridgeError.coreUnavailable
    }
}

#endif
