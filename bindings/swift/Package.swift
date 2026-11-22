// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "MmDlpCore",
    platforms: [
        .macOS(.v12),
        .iOS(.v15),
    ],
    products: [
        .library(
            name: "MmDlpCore",
            targets: ["MmDlpCore"]
        ),
    ],
    targets: [
        .target(
            name: "MmDlpCore",
            dependencies: ["MmDlpCoreFFI"],
            path: "Sources/MmDlpCore"
        ),
        .binaryTarget(
            name: "MmDlpCoreFFI",
            path: "Frameworks/MmDlpCore.xcframework"
        ),
    ]
)
