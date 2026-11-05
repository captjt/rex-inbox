// swift-tools-version: 6.0
// Rex Inbox: the Mac menu bar app for rex-inbox. ./build.sh builds it into
// ~/Applications.
import PackageDescription

let package = Package(
    name: "RexInbox",
    platforms: [.macOS(.v15)],
    targets: [
        .executableTarget(name: "RexInbox", path: "Sources/RexInbox"),
    ],
    swiftLanguageModes: [.v5]
)
