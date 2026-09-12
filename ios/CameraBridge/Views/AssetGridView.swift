import SwiftUI
import UIKit

struct AssetGridView: View {
    @Environment(ConnectionCoordinator.self) private var connection
    let enqueueSelection: () -> Void
    @Binding var enqueueError: CameraBridgeError?

    private let columns = [
        GridItem(.adaptive(minimum: 112, maximum: 180), spacing: 10),
    ]

    var body: some View {
        ScrollView {
            LazyVGrid(columns: columns, spacing: 12) {
                ForEach(connection.assets) { asset in
                    AssetCell(
                        asset: asset,
                        selected: connection.selectedAssetIDs.contains(asset.id)
                    )
                    .contentShape(.rect)
                    .onTapGesture { connection.toggleSelection(of: asset) }
                    .task { await connection.loadNextPageIfNeeded(current: asset) }
                }
            }
            .padding(.horizontal)

            if connection.isLoadingPage {
                ProgressView().padding()
            }
        }
        .safeAreaInset(edge: .bottom) {
            if !connection.selectedAssetIDs.isEmpty {
                HStack {
                    Text("已选择 \(connection.selectedAssetIDs.count) 项")
                    Spacer()
                    Button("下载到相册", action: enqueueSelection)
                        .buttonStyle(.borderedProminent)
                }
                .padding()
                .background(.bar)
            }
        }
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Menu {
                    ForEach(CameraMediaFormat.allCases, id: \.self) { format in
                        Button {
                            var formats = connection.selectedFormats
                            if formats.contains(format) {
                                if formats.count > 1 { formats.remove(format) }
                            } else {
                                formats.insert(format)
                            }
                            Task { await connection.updateFormats(formats) }
                        } label: {
                            Label(
                                format.label,
                                systemImage: connection.selectedFormats.contains(format)
                                    ? "checkmark.circle.fill"
                                    : "circle"
                            )
                        }
                    }
                } label: {
                    Label("格式", systemImage: "line.3.horizontal.decrease.circle")
                }
            }
        }
        .refreshable { await connection.reloadMedia() }
        .overlay {
            if connection.assets.isEmpty && !connection.isLoadingPage {
                ContentUnavailableView(
                    "没有可显示的照片",
                    systemImage: "photo.on.rectangle.angled",
                    description: Text("检查存储卡或调整格式筛选。")
                )
            }
        }
    }
}

private struct AssetCell: View {
    @Environment(ConnectionCoordinator.self) private var connection
    let asset: CameraAsset
    let selected: Bool
    @State private var thumbnailData: Data?

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            ZStack(alignment: .topTrailing) {
                Group {
                    if let thumbnailData, let image = UIImage(data: thumbnailData) {
                        Image(uiImage: image)
                            .resizable()
                            .scaledToFill()
                    } else {
                        Rectangle()
                            .fill(.quaternary)
                            .overlay { Image(systemName: "photo").foregroundStyle(.secondary) }
                    }
                }
                .frame(height: 112)
                .clipShape(.rect(cornerRadius: 10))

                Image(systemName: selected ? "checkmark.circle.fill" : "circle")
                    .font(.title3)
                    .symbolRenderingMode(.palette)
                    .foregroundStyle(selected ? .white : .white, selected ? .blue : .black.opacity(0.35))
                    .padding(6)
            }
            Text(asset.filename)
                .font(.caption)
                .lineLimit(1)
            HStack {
                Text(asset.format.label)
                Spacer()
                Text(Int64(clamping: asset.sizeBytes), format: .byteCount(style: .file))
            }
            .font(.caption2)
            .foregroundStyle(.secondary)
        }
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(selected ? .isSelected : [])
        .task(id: asset.id) {
            thumbnailData = await connection.thumbnail(for: asset)
        }
    }
}
