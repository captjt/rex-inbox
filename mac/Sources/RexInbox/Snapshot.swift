import AppKit
import SwiftUI

/// `RexInbox --snapshot out.png [--tab agents] [--window] [--appearance light]`
/// renders the panel (or the window, with --window), with the hub's live
/// data, to a PNG at 2x and quits. It checks the layout without opening
/// anything on screen.
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
            let asWindow = args.contains("--window")
            let root: AnyView = asWindow
                ? AnyView(MainWindow(store: store, selection: tab == .agents ? .agents : .inbox(.all)).frame(width: 980, height: 700))
                : AnyView(PanelView(store: store, tab: tab))
            let host = NSHostingView(rootView: root.background(Color(nsColor: .windowBackgroundColor)))
            let window = NSWindow(
                contentRect: NSRect(x: -10_000, y: -10_000, width: asWindow ? 980 : PanelView.width, height: asWindow ? 700 : 600),
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
                if !asWindow { window.setContentSize(host.fittingSize) }
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
