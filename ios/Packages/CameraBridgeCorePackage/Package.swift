// swift-tools-version: 6.2
import PackageDescription

let package = Package(
    name: "CameraBridgeCore",
    platforms: [.iOS(.v26)],
    products: [
        .library(name: "CameraBridgeCore", targets: ["CameraBridgeCore"]),
    ],
    targets: [
        .binaryTarget(
            name: "camera_ffiFFI",
            path: "Artifacts/camera_ffiFFI.xcframework"
        ),
        .target(
            name: "CameraBridgeCore",
            dependencies: ["camera_ffiFFI"],
            path: "Sources/CameraBridgeCore"
        ),
    ]
)
