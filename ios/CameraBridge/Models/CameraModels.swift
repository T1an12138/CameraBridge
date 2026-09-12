import Foundation

enum CameraMediaFormat: String, CaseIterable, Codable, Hashable, Sendable {
    case jpeg
    case nef

    var label: String {
        switch self {
        case .jpeg: "JPEG"
        case .nef: "NEF"
        }
    }
}

struct CameraDetails: Equatable, Sendable {
    let id: String
    let manufacturer: String
    let model: String
    let displayName: String
    let firmwareVersion: String?
}

struct CameraCapabilities: Equatable, Sendable {
    let profileID: String
    let displayName: String
    let supportedFormats: Set<CameraMediaFormat>
    let supportsThumbnails: Bool
    let supportsPartialDownloads: Bool
    let supportsEvents: Bool
    let maxPageSize: Int
    let maxThumbnailBytes: UInt64
}

struct CameraAsset: Identifiable, Hashable, Sendable {
    let id: String
    let captureGroupID: String
    let filename: String
    let format: CameraMediaFormat
    let sizeBytes: UInt64
    let capturedAt: Date?
    let width: Int?
    let height: Int?
    let thumbnailAvailable: Bool
}

struct CameraMediaPage: Equatable, Sendable {
    let items: [CameraAsset]
    let nextCursor: String?
    let totalItems: UInt64?
}

struct CameraConnectionConfiguration: Sendable {
    let host: String
    let port: UInt16
    let clientName: String
    let clientID: UUID
    let connectTimeoutMilliseconds: UInt64
    let requestTimeoutMilliseconds: UInt64
    let stagingRoot: URL
}

struct CameraProfileInput: Equatable, Sendable {
    var displayName: String = "Nikon Z6 II"
    var ssid: String = ""
    var host: String = "192.168.1.1"
    var port: Int = 15740
}

enum ConnectionStep: Equatable, Sendable {
    case disconnected
    case preparingCamera
    case joiningHotspot
    case openingSession
    case readingStorage
    case connected
    case failed(CameraBridgeError)

    var isConnected: Bool { self == .connected }
    var isWorking: Bool {
        switch self {
        case .preparingCamera, .joiningHotspot, .openingSession, .readingStorage:
            true
        default:
            false
        }
    }
}

enum CameraBridgeError: Error, Equatable, LocalizedError, Sendable {
    case passwordRejected
    case hotspotMismatch
    case localNetworkDenied
    case cameraAsleep
    case cameraBusy
    case unsupportedFirmware
    case protocolFailure(String)
    case insufficientSpace
    case sessionInterrupted
    case photosDenied
    case coreUnavailable
    case invalidConfiguration(String)
    case other(String)

    var errorDescription: String? {
        switch self {
        case .passwordRejected: String(localized: "Wi‑Fi 密码不正确，请在相机上重新确认。")
        case .hotspotMismatch: String(localized: "当前热点与相机不匹配。")
        case .localNetworkDenied: String(localized: "本地网络权限被拒绝，请前往系统设置允许。")
        case .cameraAsleep: String(localized: "相机已休眠，请唤醒相机后重试。")
        case .cameraBusy: String(localized: "相机正忙，请稍后重试。")
        case .unsupportedFirmware: String(localized: "当前固件尚未通过兼容性验证。")
        case let .protocolFailure(message): String(localized: "协议异常：\(message)")
        case .insufficientSpace: String(localized: "设备可用空间不足。")
        case .sessionInterrupted: String(localized: "相机会话已中断，请重新连接。")
        case .photosDenied: String(localized: "没有相册写入权限，原文件会保留供导出。")
        case .coreUnavailable: String(localized: "Rust 相机核心尚未生成，请先构建 XCFramework。")
        case let .invalidConfiguration(message): message
        case let .other(message): message
        }
    }
}

enum CameraTransferPhase: String, Codable, Equatable, Sendable {
    case queued
    case downloading
    case finalizing
    case completed
    case cancelled
    case failed

    var isTerminal: Bool {
        switch self {
        case .completed, .cancelled, .failed: true
        default: false
        }
    }
}

struct CameraTransferProgress: Equatable, Sendable {
    let phase: CameraTransferPhase
    let bytesTransferred: UInt64
    let totalBytes: UInt64
    let destinationURL: URL?
    let error: CameraBridgeError?

    var fractionCompleted: Double {
        guard totalBytes > 0 else { return 0 }
        return min(1, Double(bytesTransferred) / Double(totalBytes))
    }
}

struct DownloadedAssetFile: Hashable, Sendable {
    let asset: CameraAsset
    let url: URL
}

enum PhotoImportResult: Equatable, Sendable {
    case imported
    case jpegOnly(nefURL: URL)
    case retainedForFiles([URL], reason: CameraBridgeError)
}
