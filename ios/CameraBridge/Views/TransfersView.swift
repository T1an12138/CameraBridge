import SwiftUI
import UIKit

struct TransfersView: View {
    @Environment(TransferQueue.self) private var transfers
    @State private var exportRequest: ExportRequest?
    let openSettings: () -> Void

    var body: some View {
        NavigationStack {
            Group {
                if transfers.items.isEmpty {
                    ContentUnavailableView(
                        "暂无传输",
                        systemImage: "arrow.down.circle",
                        description: Text("在“相机”中选择照片后开始下载。")
                    )
                } else {
                    List(transfers.items) { item in
                        TransferRow(item: item) {
                            transfers.cancel(item.id)
                        } export: { url in
                            exportRequest = ExportRequest(itemID: item.id, url: url)
                        }
                    }
                }
            }
            .sheet(item: $exportRequest) { request in
                FileExportPicker(url: request.url) { success in
                    if success { transfers.markExported(request.itemID) }
                    exportRequest = nil
                }
            }
            .navigationTitle("传输")
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    if transfers.canClearFinished {
                        Button("清除已完成") { transfers.clearFinished() }
                    }
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button(action: openSettings) { Image(systemName: "gearshape") }
                        .accessibilityLabel("设置")
                }
            }
        }
    }
}

private struct ExportRequest: Identifiable {
    let itemID: UUID
    let url: URL
    var id: UUID { itemID }
}

private struct TransferRow: View {
    let item: TransferItemState
    let cancel: () -> Void
    let export: (URL) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .firstTextBaseline) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(item.asset.filename).font(.headline)
                    Text(item.progress.phase.label)
                        .font(.caption)
                        .foregroundStyle(item.progress.phase == .failed ? .red : .secondary)
                }
                Spacer()
                Text(item.asset.format.label)
                    .font(.caption.monospaced())
                    .padding(.horizontal, 7)
                    .padding(.vertical, 3)
                    .background(.quaternary, in: .capsule)
            }

            ProgressView(value: item.progress.fractionCompleted)
            HStack {
                Text(Int64(clamping: item.progress.bytesTransferred), format: .byteCount(style: .file))
                Text("/")
                Text(Int64(clamping: item.progress.totalBytes), format: .byteCount(style: .file))
                Spacer()
                if let localURL = item.localURL {
                    Button {
                        export(localURL)
                    } label: {
                        Label("导出 Files", systemImage: "square.and.arrow.up")
                    }
                } else if !item.progress.phase.isTerminal {
                    Button("取消", role: .destructive, action: cancel)
                }
            }
            .font(.caption)

            if let note = item.note {
                Text(note).font(.caption).foregroundStyle(.secondary)
            } else if let error = item.progress.error {
                Text(error.localizedDescription).font(.caption).foregroundStyle(.red)
            }
        }
        .padding(.vertical, 4)
        .accessibilityElement(children: .combine)
    }
}

@MainActor
private struct FileExportPicker: UIViewControllerRepresentable {
    let url: URL
    let completion: @MainActor (Bool) -> Void

    func makeCoordinator() -> Coordinator {
        Coordinator(completion: completion)
    }

    func makeUIViewController(context: Context) -> UIDocumentPickerViewController {
        let picker = UIDocumentPickerViewController(forExporting: [url], asCopy: true)
        picker.delegate = context.coordinator
        return picker
    }

    func updateUIViewController(_ controller: UIDocumentPickerViewController, context: Context) {}

    @MainActor
    final class Coordinator: NSObject, UIDocumentPickerDelegate {
        private let completion: @MainActor (Bool) -> Void

        init(completion: @escaping @MainActor (Bool) -> Void) {
            self.completion = completion
        }

        func documentPicker(_ controller: UIDocumentPickerViewController, didPickDocumentsAt urls: [URL]) {
            completion(!urls.isEmpty)
        }

        func documentPickerWasCancelled(_ controller: UIDocumentPickerViewController) {
            completion(false)
        }
    }
}

private extension CameraTransferPhase {
    var label: String {
        switch self {
        case .queued: "等待中"
        case .downloading: "正在下载"
        case .finalizing: "正在校验文件"
        case .completed: "已完成"
        case .cancelled: "已取消"
        case .failed: "失败"
        }
    }
}

#Preview {
    PreviewRoot { TransfersView {} }
}
