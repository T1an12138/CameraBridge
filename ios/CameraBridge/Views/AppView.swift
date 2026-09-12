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
            CameraView { sheet = .settings }
                .tag(AppTab.camera)
                .tabItem { Label("相机", systemImage: "camera") }

            TransfersView { sheet = .settings }
                .tag(AppTab.transfers)
                .tabItem { Label("传输", systemImage: "arrow.down.circle") }
        }
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
    case transfers
}

private enum AppSheet: String, Identifiable {
    case settings
    var id: String { rawValue }
}

#Preview {
    PreviewRoot { AppView() }
}
