import AppKit
import SwiftUI

/// `RexInbox --snapshot out.png [--tab agents] [--appearance light]` renders
/// the panel, with the hub's live data, to a PNG at 2x and quits. It checks
/// the layout without opening the menu.
@MainActor
enum Snapshot {
    static let isActive = CommandLine.arguments.contains("--snapshot")

    static func runIfAsked(_ store: Store) {
        let args = CommandLine.arguments
        guard let i = args.firstIndex(of: "--snapshot"), i + 1 < args.count else { return }
        let url = URL(fileURLWithPath: args[i + 1])
        let tab: Tab = args.contains("agents") ? .agents : .inbox
        Task { @MainActor in
            try? await Task.sleep(for: .seconds(1.5))
            await store.refresh()
            let host = NSHostingView(rootView: PanelView(store: store, tab: tab).background(Color(nsColor: .windowBackgroundColor)))
            let window = NSWindow(
                contentRect: NSRect(x: -10_000, y: -10_000, width: 440, height: 600),
                styleMask: [.borderless], backing: .buffered, defer: false
            )
            if let j = args.firstIndex(of: "--appearance"), j + 1 < args.count {
                window.appearance = NSAppearance(named: args[j + 1] == "light" ? .aqua : .darkAqua)
            }
            window.backgroundColor = .windowBackgroundColor
            window.contentView = host
            window.orderFrontRegardless()
            for _ in 0..<3 {
                try? await Task.sleep(for: .seconds(0.5))
                window.setContentSize(host.fittingSize)
            }
            // Off screen the backing scale is 1; draw at 2x to stay sharp.
            let size = host.bounds.size
            if let rep = NSBitmapImageRep(
                bitmapDataPlanes: nil, pixelsWide: Int(size.width * 2), pixelsHigh: Int(size.height * 2),
                bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0
            ) {
                rep.size = size
                host.cacheDisplay(in: host.bounds, to: rep)
                try? rep.representation(using: .png, properties: [:])?.write(to: url)
            }
            NSApp.terminate(nil)
        }
    }
}
