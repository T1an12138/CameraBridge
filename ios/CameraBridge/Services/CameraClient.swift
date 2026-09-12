import Foundation

protocol CameraCoreClient: Sendable {
    func connect(configuration: CameraConnectionConfiguration) async throws -> any CameraSessionClient
}

protocol CameraSessionClient: Sendable {
    func info() async throws -> CameraDetails
    func capabilities() async throws -> CameraCapabilities
    func listMedia(
        formats: Set<CameraMediaFormat>,
        pageSize: Int,
        cursor: String?
    ) async throws -> CameraMediaPage
    func loadThumbnail(assetID: String, maxBytes: UInt64) async throws -> Data
    func startDownload(asset: CameraAsset, filename: String, overwrite: Bool) async throws -> any CameraTransferClient
    func disconnect() async throws
}

protocol CameraTransferClient: Sendable {
    func snapshot() async -> CameraTransferProgress
    func cancel() async
    func wait() async -> CameraTransferProgress
}
