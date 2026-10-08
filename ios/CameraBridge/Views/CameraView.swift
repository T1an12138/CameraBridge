import SwiftData
import SwiftUI

struct CameraView: View {
    @Environment(ConnectionCoordinator.self) private var connection
    @Environment(\.modelContext) private var modelContext
    @Query(sort: \SavedCameraProfile.lastConnectedAt, order: .reverse) private var savedProfiles: [SavedCameraProfile]

    @AppStorage("CameraBridge.LastSSID") private var lastSSID = ""
    @AppStorage("CameraBridge.LastHost") private var lastHost = "192.168.1.1"
    @AppStorage("CameraBridge.LastPort") private var lastPort = 15740
    @State private var profile = CameraProfileInput()
    @State private var password = ""

    let openSettings: () -> Void
    let openPhotos: () -> Void

    var body: some View {
        NavigationStack {
            Group {
                if connection.step.isConnected {
                    List {
                        Section {
                            Label {
                                VStack(alignment: .leading, spacing: 6) {
                                    Text(connection.camera?.displayName ?? "Nikon Z6 II").font(.headline)
                                    Text(connection.camera?.firmwareVersion ?? "C 1.70").font(.subheadline).foregroundStyle(.secondary)
                                }
                            } icon: {
                                Image(systemName: "camera").font(.title2).foregroundStyle(.primary)
                            }
                            Label("相机已连接", systemImage: "checkmark.circle.fill").foregroundStyle(.secondary)
                        }
                        Section {
                            Button("浏览照片", action: openPhotos)
                                .frame(maxWidth: .infinity)
                                .buttonStyle(.borderedProminent)
                                .controlSize(.large)
                        }
                        .listRowBackground(Color.clear)
                        .listRowInsets(EdgeInsets())
                    }
                    .listStyle(.insetGrouped)
                } else {
                    ConnectionView(
                        profile: $profile,
                        password: $password,
                        connect: connect
                    )
                }
            }
            .navigationTitle("相机")
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

}

private struct ConnectionView: View {
    @Environment(ConnectionCoordinator.self) private var connection
    @Binding var profile: CameraProfileInput
    @Binding var password: String
    let connect: () -> Void

    var body: some View {
        Form {
            Section {
                Label {
                    VStack(alignment: .leading, spacing: 6) {
                        Text(profile.displayName).font(.headline)
                        Text("C 1.70 · Wi-Fi / PTP-IP").font(.subheadline).foregroundStyle(.secondary)
                    }
                } icon: {
                    Image(systemName: "camera").font(.title2).foregroundStyle(.primary)
                }
            }
            #if targetEnvironment(simulator)
            Section {
                Label("演示相机", systemImage: "play.rectangle")
            } footer: {
                Text("当前是模拟器协议测试，未读取实体相机，不提供照片预览。")
            }
            #else
            Section("在相机上") {
                Label("网络菜单 → 连接至智能设备 → Wi‑Fi 连接", systemImage: "1.circle")
                Label("保持相机屏幕唤醒并停留在网络页面", systemImage: "2.circle")
            }

            Section {
                TextField("相机显示的 SSID", text: $profile.ssid)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                SecureField("Wi‑Fi 密码（留空使用已保存密码）", text: $password)
            } header: {
                Text("相机热点")
            } footer: {
                Text("填写相机显示的热点名称，并保持相机唤醒。密码可留空使用已保存的配置。")
            }
            #endif

            Section {
                DisclosureGroup("高级连接设置") {
                    TextField("显示名称", text: $profile.displayName)
                    TextField("相机地址", text: $profile.host)
                        .textInputAutocapitalization(.never)
                        .keyboardType(.numbersAndPunctuation)
                    TextField("PTP/IP 端口", value: $profile.port, format: .number.grouping(.never))
                        .keyboardType(.numberPad)
                }
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
                .buttonStyle(.borderedProminent)
                .controlSize(.large)
            }
            .listRowBackground(Color.clear)
            .listRowInsets(EdgeInsets())

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
