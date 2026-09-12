import SwiftData
import SwiftUI

@main
@MainActor
struct CameraBridgeApp: App {
    @State private var connection: ConnectionCoordinator
    @State private var transfers: TransferQueue
    private let modelContainer: ModelContainer

    init() {
        var stagingRoot = FileManager.default.urls(
            for: .applicationSupportDirectory,
            in: .userDomainMask
        )[0]
            .appending(path: "CameraBridge/Staging", directoryHint: .isDirectory)
        do {
            try FileManager.default.createDirectory(
                at: stagingRoot,
                withIntermediateDirectories: true
            )
        } catch {
            fatalError("无法创建 CameraBridge 暂存目录：\(error)")
        }
        var resourceValues = URLResourceValues()
        resourceValues.isExcludedFromBackup = true
        try? stagingRoot.setResourceValues(resourceValues)

        let useMockCamera: Bool
        #if targetEnvironment(simulator)
        useMockCamera = true
        #else
        useMockCamera = ProcessInfo.processInfo.environment["CAMERABRIDGE_USE_MOCK"] == "1"
        #endif

        let core: any CameraCoreClient
        let hotspotJoiner: any HotspotJoining
        if useMockCamera {
            core = MockCameraCore()
            hotspotJoiner = PreviewHotspotJoiner()
        } else {
            core = RustCameraCore()
            hotspotJoiner = SystemHotspotJoiner()
        }

        _connection = State(
            initialValue: ConnectionCoordinator(
                core: core,
                hotspotJoiner: hotspotJoiner,
                keychain: KeychainStore(),
                stagingRoot: stagingRoot
            )
        )
        _transfers = State(initialValue: TransferQueue(photos: PhotoLibraryImporter()))

        do {
            modelContainer = try ModelContainer(for: SavedCameraProfile.self, TransferRecord.self)
        } catch {
            fatalError("无法初始化 CameraBridge 本地存储：\(error)")
        }
    }

    var body: some Scene {
        WindowGroup {
            AppView()
                .environment(connection)
                .environment(transfers)
        }
        .modelContainer(modelContainer)
    }
}
