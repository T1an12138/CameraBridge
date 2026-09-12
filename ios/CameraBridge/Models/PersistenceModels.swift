import Foundation
import SwiftData

@Model
final class SavedCameraProfile {
    @Attribute(.unique) var id: UUID
    var displayName: String
    var ssid: String
    var host: String
    var port: Int
    var createdAt: Date
    var lastConnectedAt: Date?

    init(
        id: UUID = UUID(),
        displayName: String,
        ssid: String,
        host: String,
        port: Int,
        createdAt: Date = .now,
        lastConnectedAt: Date? = nil
    ) {
        self.id = id
        self.displayName = displayName
        self.ssid = ssid
        self.host = host
        self.port = port
        self.createdAt = createdAt
        self.lastConnectedAt = lastConnectedAt
    }
}

@Model
final class TransferRecord {
    @Attribute(.unique) var id: UUID
    var cameraAssetID: String
    var captureGroupID: String
    var filename: String
    var formatRawValue: String
    var statusRawValue: String
    var bytesTransferred: Int64
    var totalBytes: Int64
    var localPath: String?
    var photosImported: Bool
    var exportedToFiles: Bool
    var completedAt: Date?
    var preserveUntil: Date?
    var errorMessage: String?

    init(id: UUID = UUID(), asset: CameraAsset) {
        self.id = id
        cameraAssetID = asset.id
        captureGroupID = asset.captureGroupID
        filename = asset.filename
        formatRawValue = asset.format.rawValue
        statusRawValue = CameraTransferPhase.queued.rawValue
        bytesTransferred = 0
        totalBytes = Int64(clamping: asset.sizeBytes)
        localPath = nil
        photosImported = false
        exportedToFiles = false
        completedAt = nil
        preserveUntil = nil
        errorMessage = nil
    }
}
