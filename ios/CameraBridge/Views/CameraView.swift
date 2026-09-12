import SwiftData
import SwiftUI

struct CameraView: View {
    @Environment(ConnectionCoordinator.self) private var connection
    @Environment(TransferQueue.self) private var transfers
    @Environment(\.modelContext) private var modelContext
    @Query(sort: \SavedCameraProfile.lastConnectedAt, order: .reverse) private var savedProfiles: [SavedCameraProfile]

    @AppStorage("CameraBridge.LastSSID") private var lastSSID = ""
    @AppStorage("CameraBridge.LastHost") private var lastHost = "192.168.1.1"
    @AppStorage("CameraBridge.LastPort") private var lastPort = 15740
    @State private var profile = CameraProfileInput()
    @State private var password = ""
    @State private var enqueueError: CameraBridgeError?

    let openSettings: () -> Void

    var body: some View {
        NavigationStack {
            Group {
                if connection.step.isConnected {
                    AssetGridView(
                        enqueueSelection: enqueueSelection,
                        enqueueError: $enqueueError
                    )
                } else {
                    ConnectionView(
                        profile: $profile,
                        password: $password,
                        connect: connect
                    )
                }
            }
            .navigationTitle(connection.camera?.displayName ?? String(localized: "相机"))
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button(action: openSettings) {
                        Image(systemName: "gearshape")
                    }
                    .accessibilityLabel("设置")
                }
                if connection.step.isConnected {
                    ToolbarItem(placement: .topBarLeading) {
                        Button("断开") { Task { await connection.disconnect() } }
                    }
                }
            }
            .alert(
                "无法开始传输",
                isPresented: Binding(
                    get: { enqueueError != nil },
                    set: { if !$0 { enqueueError = nil } }
                ),
                presenting: enqueueError
            ) { _ in
                Button("好", role: .cancel) {}
            } message: { error in
                Text(error.localizedDescription)
            }
            .task {
                let latest = savedProfiles.first
                let suggestedSSID: String
                #if targetEnvironment(simulator)
                suggestedSSID = latest?.ssid ?? (lastSSID.isEmpty ? "Nikon-Z6II-Mock" : lastSSID)
                #else
                suggestedSSID = latest?.ssid ?? lastSSID
                #endif
                profile = CameraProfileInput(
                    displayName: latest?.displayName ?? "Nikon Z6 II",
                    ssid: suggestedSSID,
                    host: latest?.host ?? lastHost,
                    port: latest?.port ?? lastPort
                )
            }
        }
    }

    private func connect() {
        let requestedProfile = profile
        let requestedPassword = password
        lastSSID = requestedProfile.ssid
        lastHost = requestedProfile.host
        lastPort = requestedProfile.port
        Task {
            await connection.connect(profile: requestedProfile, password: requestedPassword)
            guard connection.step.isConnected else { return }
            if let existing = savedProfiles.first(where: { $0.ssid == requestedProfile.ssid }) {
                existing.displayName = requestedProfile.displayName
                existing.host = requestedProfile.host
                existing.port = requestedProfile.port
                existing.lastConnectedAt = .now
            } else {
                modelContext.insert(
                    SavedCameraProfile(
                        displayName: requestedProfile.displayName,
                        ssid: requestedProfile.ssid,
                        host: requestedProfile.host,
                        port: requestedProfile.port,
                        lastConnectedAt: .now
                    )
                )
            }
            try? modelContext.save()
            if password == requestedPassword {
                password = ""
            }
        }
    }

    private func enqueueSelection() {
        do {
            let session = try connection.connectedSession()
            try transfers.enqueue(
                assets: connection.selectedAssets(),
                session: session,
                modelContext: modelContext
            )
            connection.clearSelection()
        } catch {
            enqueueError = error as? CameraBridgeError ?? .other(error.localizedDescription)
        }
    }
}

private struct ConnectionView: View {
    @Environment(ConnectionCoordinator.self) private var connection
    @Binding var profile: CameraProfileInput
    @Binding var password: String
    let connect: () -> Void

    var body: some View {
        Form {
            #if targetEnvironment(simulator)
            Section("模拟器演示") {
                Label("当前使用内置 Nikon Z6 II 模拟相机，无需连接真实热点。", systemImage: "shippingbox")
            }
            #endif
            Section("在相机上") {
                Label("网络菜单 → 连接至智能设备 → Wi‑Fi 连接", systemImage: "1.circle")
                Label("保持相机屏幕唤醒并停留在网络页面", systemImage: "2.circle")
            }

            Section("相机热点") {
                TextField("显示名称", text: $profile.displayName)
                TextField("相机显示的 SSID", text: $profile.ssid)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                SecureField("Wi‑Fi 密码（留空使用已保存密码）", text: $password)
                TextField("相机地址", text: $profile.host)
                    .textInputAutocapitalization(.never)
                    .keyboardType(.numbersAndPunctuation)
                TextField(
                    "PTP/IP 端口",
                    value: $profile.port,
                    format: .number.grouping(.never)
                )
                .keyboardType(.numberPad)
            } footer: {
                Text("iOS 不允许普通 App 扫描附近 SSID；地址只做固定单播，不扫描子网。")
            }

            Section {
                Button(action: connect) {
                    HStack {
                        Spacer()
                        if connection.step.isWorking {
                            ProgressView().padding(.trailing, 6)
                        }
                        Text(connection.step.isWorking ? connection.step.statusText : "连接相机")
                        Spacer()
                    }
                }
                .disabled(connection.step.isWorking || profile.ssid.isEmpty)
            }

            if case let .failed(error) = connection.step {
                Section("需要处理") {
                    Label(error.localizedDescription, systemImage: "exclamationmark.triangle")
                        .foregroundStyle(.orange)
                }
            }
        }
    }
}

private extension ConnectionStep {
    var statusText: String {
        switch self {
        case .disconnected: "未连接"
        case .preparingCamera: "准备相机"
        case .joiningHotspot: "加入热点"
        case .openingSession: "打开会话"
        case .readingStorage: "读取存储卡"
        case .connected: "已连接"
        case .failed: "连接失败"
        }
    }
}
