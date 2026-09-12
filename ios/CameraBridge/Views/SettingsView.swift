import SwiftData
import SwiftUI

struct SettingsView: View {
    @Environment(\.dismiss) private var dismiss
    @Query(sort: \SavedCameraProfile.lastConnectedAt, order: .reverse) private var profiles: [SavedCameraProfile]
    @Query(sort: \TransferRecord.completedAt, order: .reverse) private var transferRecords: [TransferRecord]

    var body: some View {
        List {
            Section("相机配置") {
                if profiles.isEmpty {
                    Text("首次连接后会记住 SSID 和地址；密码仅保存在 Keychain。")
                        .foregroundStyle(.secondary)
                } else {
                    ForEach(profiles) { profile in
                        VStack(alignment: .leading, spacing: 3) {
                            Text(profile.displayName)
                            Text("\(profile.ssid) · \(profile.host):\(profile.port)")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                    }
                }
            }

            Section("文件保留") {
                LabeledContent("传输记录", value: "\(transferRecords.count)")
                Text("完成的原文件至少保留 24 小时。失败或尚未导出的文件不会自动删除。")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }

            Section("隐私") {
                Label("无账号、无云端、无遥测", systemImage: "hand.raised")
                Label("只在前台连接相机热点", systemImage: "wifi")
                Label("不申请后台或多播权限", systemImage: "checkmark.shield")
            }

            Section("当前支持") {
                LabeledContent("机型", value: "Nikon Z6 II")
                LabeledContent("固件验证目标", value: "C 1.70")
                LabeledContent("格式", value: "JPEG / NEF")
                Text("其他 Nikon Z 系列和其他品牌将在 Z6 II 真机协议验证完成后逐步接入。")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
        }
        .navigationTitle("设置")
        .toolbar {
            ToolbarItem(placement: .confirmationAction) {
                Button("完成") { dismiss() }
            }
        }
    }
}
