import SwiftData
import SwiftUI

@MainActor
struct AppView: View {
    @Environment(\.scenePhase) private var scenePhase
    @Environment(\.modelContext) private var modelContext
    @Environment(ConnectionCoordinator.self) private var connection
    @Environment(TransferQueue.self) private var transfers
    @Query private var transferRecords: [TransferRecord]
    @State private var selectedTab: AppTab = .camera
    @State private var sheet: AppSheet?
    @State private var storageError: String?
    private let retention = StagingRetentionService()

    var body: some View {
        TabView(selection: $selectedTab) {
            CameraView(openSettings: { sheet = .settings }, openPhotos: { selectedTab = .photos })
                .tag(AppTab.camera)
                .tabItem { Label("相机", systemImage: "camera") }

            CameraPhotosView { sheet = .settings }
                .tag(AppTab.photos)
                .tabItem { Label("照片", systemImage: "photo.on.rectangle") }

            TransfersView { sheet = .settings }
                .tag(AppTab.transfers)
                .tabItem { Label("传输", systemImage: "arrow.down.circle") }

            NavigationStack { SettingsView(showsDoneButton: false) }
                .tag(AppTab.settings)
                .tabItem { Label("设置", systemImage: "gearshape") }
        }
        .tint(.blue)
        .sheet(item: $sheet) { route in
            switch route {
            case .settings:
                NavigationStack { SettingsView() }
            }
        }
        .task {
            retention.removeEligibleFiles(records: transferRecords, modelContext: modelContext)
            do {
                try transfers.restore(modelContext: modelContext)
            } catch {
                storageError = error.localizedDescription
            }
            #if targetEnvironment(simulator)
            if ProcessInfo.processInfo.environment["CAMERABRIDGE_PREVIEW_PHOTOS"] == "1" {
                await connection.connect(profile: CameraProfileInput(ssid: "Nikon-Z6II-Mock"), password: "")
                if connection.step.isConnected { selectedTab = .photos }
            }
            #endif
        }
        .alert("无法读取传输记录", isPresented: Binding(
            get: { storageError != nil },
            set: { if !$0 { storageError = nil } }
        )) {
            Button("好", role: .cancel) {}
        } message: {
            Text(storageError ?? "")
        }
        .onChange(of: scenePhase) { _, newValue in
            if newValue == .active {
                connection.noteForegroundReturn()
                retention.removeEligibleFiles(records: transferRecords, modelContext: modelContext)
            }
        }
    }
}

private enum AppTab: Hashable {
    case camera
    case photos
    case transfers
    case settings
}

private enum AppSheet: String, Identifiable {
    case settings
    var id: String { rawValue }
}

#Preview {
    PreviewRoot { AppView() }
}
