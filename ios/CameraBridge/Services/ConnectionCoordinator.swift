import Foundation
import Observation

@MainActor
@Observable
final class ConnectionCoordinator {
    private(set) var step: ConnectionStep = .disconnected
    private(set) var camera: CameraDetails?
    private(set) var capabilities: CameraCapabilities?
    private(set) var assets: [CameraAsset] = []
    private(set) var nextCursor: String?
    private(set) var totalItems: UInt64?
    private(set) var isLoadingPage = false
    var selectedAssetIDs: Set<String> = []
    var selectedFormats: Set<CameraMediaFormat> = Set(CameraMediaFormat.allCases)

    @ObservationIgnored private let core: any CameraCoreClient
    @ObservationIgnored private let hotspotJoiner: any HotspotJoining
    @ObservationIgnored private let keychain: KeychainStore
    @ObservationIgnored private let stagingRoot: URL
    @ObservationIgnored private var session: (any CameraSessionClient)?
    @ObservationIgnored private var connectedSSID: String?
    @ObservationIgnored private let clientID: UUID

    init(
        core: any CameraCoreClient,
        hotspotJoiner: any HotspotJoining,
        keychain: KeychainStore,
        stagingRoot: URL
    ) {
        self.core = core
        self.hotspotJoiner = hotspotJoiner
        self.keychain = keychain
        self.stagingRoot = stagingRoot
        clientID = Self.loadClientID()
    }

    func savedPassword(forSSID ssid: String) async -> String? {
        try? await keychain.password(forSSID: ssid)
    }

    func connect(profile: CameraProfileInput, password suppliedPassword: String) async {
        guard !step.isWorking else { return }
        guard (1...65_535).contains(profile.port),
              let port = UInt16(exactly: profile.port),
              !profile.host.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            step = .failed(.invalidConfiguration(String(localized: "相机地址或端口无效。")))
            return
        }

        let password: String
        if suppliedPassword.isEmpty {
            password = (try? await keychain.password(forSSID: profile.ssid)) ?? ""
        } else {
            password = suppliedPassword
        }

        do {
            step = .preparingCamera
            try await Task.sleep(for: .milliseconds(120))
            step = .joiningHotspot
            try await hotspotJoiner.join(ssid: profile.ssid, password: password)
            connectedSSID = profile.ssid

            step = .openingSession
            let openedSession = try await core.connect(
                configuration: CameraConnectionConfiguration(
                    host: profile.host,
                    port: port,
                    clientName: "CameraBridge iOS",
                    clientID: clientID,
                    connectTimeoutMilliseconds: 8_000,
                    requestTimeoutMilliseconds: 20_000,
                    stagingRoot: stagingRoot
                )
            )
            session = openedSession

            step = .readingStorage
            async let cameraInfo = openedSession.info()
            async let cameraCapabilities = openedSession.capabilities()
            camera = try await cameraInfo
            capabilities = try await cameraCapabilities
            assets = []
            nextCursor = nil
            totalItems = nil
            selectedAssetIDs = []
            try await loadPage(reset: true)
            if !password.isEmpty {
                try await keychain.savePassword(password, forSSID: profile.ssid)
            }
            step = .connected
        } catch {
            if let session {
                try? await session.disconnect()
            }
            session = nil
            camera = nil
            capabilities = nil
            step = .failed(Self.map(error))
        }
    }

    func disconnect() async {
        if let session { try? await session.disconnect() }
        if let connectedSSID { await hotspotJoiner.removeConfiguration(ssid: connectedSSID) }
        session = nil
        connectedSSID = nil
        camera = nil
        capabilities = nil
        assets = []
        nextCursor = nil
        totalItems = nil
        selectedAssetIDs = []
        step = .disconnected
    }

    func reloadMedia() async {
        do {
            try await loadPage(reset: true)
        } catch {
            step = .failed(Self.map(error))
        }
    }

    func loadNextPageIfNeeded(current asset: CameraAsset) async {
        guard asset.id == assets.last?.id, nextCursor != nil else { return }
        do {
            try await loadPage(reset: false)
        } catch {
            step = .failed(Self.map(error))
        }
    }

    func updateFormats(_ formats: Set<CameraMediaFormat>) async {
        selectedFormats = formats
        await reloadMedia()
    }

    func thumbnail(for asset: CameraAsset) async -> Data? {
        guard asset.thumbnailAvailable, let session else { return nil }
        let maximum = capabilities?.maxThumbnailBytes ?? 2_000_000
        return try? await session.loadThumbnail(assetID: asset.id, maxBytes: maximum)
    }

    func toggleSelection(of asset: CameraAsset) {
        if selectedAssetIDs.contains(asset.id) {
            selectedAssetIDs.remove(asset.id)
        } else {
            selectedAssetIDs.insert(asset.id)
        }
    }

    func clearSelection() { selectedAssetIDs = [] }

    func selectedAssets() -> [CameraAsset] {
        assets.filter { selectedAssetIDs.contains($0.id) }
    }

    func connectedSession() throws -> any CameraSessionClient {
        guard let session, step.isConnected else { throw CameraBridgeError.sessionInterrupted }
        return session
    }

    func noteForegroundReturn() {
        if step.isConnected {
            // 进入后台后，joinOnce 配置可能已被移除；下一次 PTP 操作
            // 才是判断连接是否仍然有效的权威探针。
            step = .connected
        }
    }

    private func loadPage(reset: Bool) async throws {
        guard !isLoadingPage, let session else { return }
        if !reset, nextCursor == nil { return }
        isLoadingPage = true
        defer { isLoadingPage = false }

        let page = try await session.listMedia(
            formats: selectedFormats,
            pageSize: min(capabilities?.maxPageSize ?? 60, 60),
            cursor: reset ? nil : nextCursor
        )
        let combined = reset ? page.items : assets + page.items
        assets = combined.sorted {
            ($0.capturedAt ?? .distantPast) > ($1.capturedAt ?? .distantPast)
        }
        nextCursor = page.nextCursor
        totalItems = page.totalItems
    }

    private static func loadClientID() -> UUID {
        let key = "CameraBridge.PTPIPClientID"
        if let value = UserDefaults.standard.string(forKey: key), let id = UUID(uuidString: value) {
            return id
        }
        let id = UUID()
        UserDefaults.standard.set(id.uuidString, forKey: key)
        return id
    }

    private static func map(_ error: Error) -> CameraBridgeError {
        if let mapped = error as? CameraBridgeError { return mapped }
        let nsError = error as NSError
        if nsError.domain == NSURLErrorDomain, nsError.code == NSURLErrorCannotConnectToHost {
            return .sessionInterrupted
        }
        return .other(error.localizedDescription)
    }
}
