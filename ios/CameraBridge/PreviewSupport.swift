import SwiftData
import SwiftUI

@MainActor
struct PreviewRoot<Content: View>: View {
    @State private var connection: ConnectionCoordinator
    @State private var transfers: TransferQueue
    private let container: ModelContainer
    private let content: Content

    init(@ViewBuilder content: () -> Content) {
        let staging = FileManager.default.temporaryDirectory
            .appending(path: "CameraBridge-Preview", directoryHint: .isDirectory)
        _connection = State(
            initialValue: ConnectionCoordinator(
                core: MockCameraCore(),
                hotspotJoiner: PreviewHotspotJoiner(),
                keychain: KeychainStore(),
                stagingRoot: staging
            )
        )
        _transfers = State(initialValue: TransferQueue(photos: PhotoLibraryImporter()))
        let configuration = ModelConfiguration(isStoredInMemoryOnly: true)
        container = try! ModelContainer(
            for: SavedCameraProfile.self,
            TransferRecord.self,
            configurations: configuration
        )
        self.content = content()
    }

    var body: some View {
        content
            .environment(connection)
            .environment(transfers)
            .modelContainer(container)
    }
}
