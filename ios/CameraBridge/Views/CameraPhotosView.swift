import SwiftData
import SwiftUI

struct CameraPhotosView: View {
    @Environment(ConnectionCoordinator.self) private var connection
    @Environment(TransferQueue.self) private var transfers
    @Environment(\.modelContext) private var modelContext
    @State private var enqueueError: CameraBridgeError?
    let openSettings: () -> Void

    var body: some View {
        NavigationStack {
            Group {
                if connection.step.isConnected {
                    AssetGridView(enqueueSelection: enqueueSelection, enqueueError: $enqueueError)
                } else {
                    ContentUnavailableView("还没有连接相机", systemImage: "photo.on.rectangle",
                        description: Text("在相机页面连接设备后浏览照片。"))
                }
            }
            .navigationTitle("照片")
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button(action: openSettings) { Image(systemName: "gearshape") }
                        .accessibilityLabel("设置")
                }
            }
            .alert("无法开始传输", isPresented: Binding(
                get: { enqueueError != nil }, set: { if !$0 { enqueueError = nil } }
            ), presenting: enqueueError) { _ in
                Button("好", role: .cancel) {}
            } message: { error in
                Text(error.localizedDescription)
            }
        }
    }

    private func enqueueSelection() {
        do {
            try transfers.enqueue(assets: connection.selectedAssets(), session: connection.connectedSession(), modelContext: modelContext)
            connection.clearSelection()
        } catch {
            enqueueError = error as? CameraBridgeError ?? .other(error.localizedDescription)
        }
    }
}
