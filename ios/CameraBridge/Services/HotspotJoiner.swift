import Foundation
import NetworkExtension

protocol HotspotJoining: Sendable {
    func join(ssid: String, password: String) async throws
    func removeConfiguration(ssid: String) async
}

struct SystemHotspotJoiner: HotspotJoining {
    func join(ssid: String, password: String) async throws {
        guard !ssid.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            throw CameraBridgeError.invalidConfiguration(String(localized: "请输入相机显示的 Wi‑Fi 名称。"))
        }
        guard password.count >= 8 else {
            throw CameraBridgeError.passwordRejected
        }

        let configuration = NEHotspotConfiguration(ssid: ssid, passphrase: password, isWEP: false)
        configuration.joinOnce = true

        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, any Error>) in
            NEHotspotConfigurationManager.shared.apply(configuration) { error in
                guard let nsError = error as NSError? else {
                    continuation.resume()
                    return
                }
                if nsError.domain == NEHotspotConfigurationErrorDomain,
                   nsError.code == NEHotspotConfigurationError.alreadyAssociated.rawValue {
                    continuation.resume()
                    return
                }
                if nsError.domain == NEHotspotConfigurationErrorDomain,
                   nsError.code == NEHotspotConfigurationError.invalidWPAPassphrase.rawValue {
                    continuation.resume(throwing: CameraBridgeError.passwordRejected)
                    return
                }
                continuation.resume(throwing: CameraBridgeError.hotspotMismatch)
            }
        }
    }

    func removeConfiguration(ssid: String) async {
        NEHotspotConfigurationManager.shared.removeConfiguration(forSSID: ssid)
    }
}

struct PreviewHotspotJoiner: HotspotJoining {
    func join(ssid: String, password: String) async throws {
        try await Task.sleep(for: .milliseconds(100))
    }

    func removeConfiguration(ssid: String) async {}
}
