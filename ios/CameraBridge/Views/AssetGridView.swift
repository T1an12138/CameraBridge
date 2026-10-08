import SwiftUI
import UIKit

struct AssetGridView: View {
    @Environment(ConnectionCoordinator.self) private var connection
    let enqueueSelection: () -> Void
    @Binding var enqueueError: CameraBridgeError?
    private let columns = [GridItem(.adaptive(minimum: 150, maximum: 230), spacing: 12)]

    var body: some View {
        VStack(spacing: 12) {
            VStack(alignment: .leading, spacing: 12) {
                Text("已加载 \(connection.assets.count) / \(connection.totalItems ?? UInt64(connection.assets.count)) 个对象")
                    .font(.subheadline).foregroundStyle(.secondary)
                Picker("格式", selection: Binding(
                    get: { PhotoFilter(formats: connection.selectedFormats) },
                    set: { filter in Task { await connection.updateFormats(filter.formats) } }
                )) {
                    ForEach(PhotoFilter.allCases, id: \.self) { filter in
                        Text(filter.label).tag(filter)
                    }
                }
                .pickerStyle(.segmented)
                .disabled(connection.isLoadingPage)
            }
            .padding(.horizontal, 20)

            ScrollView {
                LazyVGrid(columns: columns, spacing: 16) {
                    ForEach(connection.assets) { asset in
                        Button { connection.toggleSelection(of: asset) } label: {
                            AssetCell(asset: asset, selected: connection.selectedAssetIDs.contains(asset.id))
                        }
                        .buttonStyle(.plain)
                        .accessibilityIdentifier("asset-\(asset.id)")
                        .task { await connection.loadNextPageIfNeeded(current: asset) }
                    }
                }
                .padding(.horizontal, 20)
                .padding(.top, 4)
                .padding(.bottom, 16)
                if connection.isLoadingPage { ProgressView().padding() }
            }
            .refreshable { await connection.reloadMedia() }
            .overlay {
                if connection.assets.isEmpty && !connection.isLoadingPage {
                    ContentUnavailableView("没有可显示的照片", systemImage: "photo.on.rectangle",
                        description: Text("检查存储卡或调整格式筛选。"))
                }
            }
        }
        .padding(.top, 8)
        .background(Color(uiColor: .systemGroupedBackground))
        .safeAreaInset(edge: .bottom) {
            HStack {
                Text("已选 \(connection.selectedAssetIDs.count) 项")
                    .font(.subheadline).foregroundStyle(.secondary)
                Spacer()
                Button("下载所选", action: enqueueSelection)
                    .buttonStyle(.borderedProminent)
                    .disabled(connection.selectedAssetIDs.isEmpty)
            }
            .padding(.horizontal, 20)
            .padding(.vertical, 12)
            .background(.bar)
        }
        .toolbar {
            ToolbarItem(placement: .topBarLeading) {
                Button(connection.allLoadedAssetsSelected ? "取消全选" : "全选") {
                    connection.toggleAllLoadedAssets()
                }
                .disabled(connection.assets.isEmpty)
            }
        }
    }
}

private enum PhotoFilter: CaseIterable, Hashable {
    case all, jpeg, nef
    init(formats: Set<CameraMediaFormat>) {
        self = formats == [.jpeg] ? .jpeg : (formats == [.nef] ? .nef : .all)
    }
    var formats: Set<CameraMediaFormat> {
        switch self {
        case .all: [.jpeg, .nef]
        case .jpeg: [.jpeg]
        case .nef: [.nef]
        }
    }
    var label: String {
        switch self {
        case .all: "全部"
        case .jpeg: "JPEG"
        case .nef: "NEF"
        }
    }
}

private struct AssetCell: View {
    @Environment(ConnectionCoordinator.self) private var connection
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    let asset: CameraAsset
    let selected: Bool
    @State private var image: UIImage?
    @State private var didLoad = false

    var body: some View {
        VStack(alignment: .leading, spacing: 7) {
            GeometryReader { geometry in
                ZStack(alignment: .topTrailing) {
                    Group {
                        if let image {
                            Image(uiImage: image).resizable().scaledToFill()
                        } else {
                            Color(uiColor: .secondarySystemGroupedBackground)
                                .overlay {
                                    if !didLoad && asset.thumbnailAvailable { ProgressView() }
                                    else { Image(systemName: "photo").foregroundStyle(.secondary) }
                                }
                        }
                    }
                    .frame(width: geometry.size.width, height: geometry.size.height)
                    .clipped()
                    ZStack {
                        Circle().fill(selected ? Color.blue : Color.black.opacity(0.25))
                        Circle().strokeBorder(.white, lineWidth: 1.5)
                        if selected { Image(systemName: "checkmark").font(.caption.bold()).foregroundStyle(.white) }
                    }
                    .frame(width: 24, height: 24)
                    .padding(8)
                }
            }
            .aspectRatio(4.0 / 3.0, contentMode: .fit)
            .clipShape(.rect(cornerRadius: 12))
            .overlay { RoundedRectangle(cornerRadius: 12).strokeBorder(selected ? Color.blue : .clear, lineWidth: 2) }
            Text(asset.filename).font(.caption.weight(.medium)).lineLimit(1)
            HStack {
                Text(asset.format.label)
                Spacer(minLength: 4)
                Text(Int64(clamping: asset.sizeBytes), format: .byteCount(style: .file))
            }
            .font(.caption2).foregroundStyle(.secondary)
        }
        .accessibilityElement(children: .combine)
        .accessibilityValue("\(selected ? "已选择" : "未选择")，\(image != nil ? "预览已加载" : "预览不可用")")
        .accessibilityAddTraits(selected ? .isSelected : [])
        .animation(reduceMotion ? nil : .easeOut(duration: 0.12), value: selected)
        .task(id: asset.id) {
            image = nil
            didLoad = false
            let data = await connection.thumbnail(for: asset)
            guard !Task.isCancelled else { return }
            image = data.flatMap { UIImage(data: $0) }
            didLoad = true
        }
    }
}
