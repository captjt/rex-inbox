import AppKit
import SwiftUI

// Rex Inbox: everything your Rex agents need from you, in the menu bar.
// It talks to the rex-inbox hub over its unix socket (see src/api.rs).

@main
struct RexInboxApp: App {
    @State private var store = Store()

    var body: some Scene {
        MenuBarExtra {
            PanelView(store: store)
        } label: {
            MenuBarLabel(store: store)
        }
        .menuBarExtraStyle(.window)

        Window("Rex Inbox", id: "main") {
            MainWindow(store: store)
        }
        .defaultSize(width: 980, height: 700)
        .defaultLaunchBehavior(.suppressed)

        Window("Review", id: "preview") {
            PreviewWindow(store: store)
        }
        .defaultSize(width: 720, height: 640)
        .defaultLaunchBehavior(.suppressed)
    }
}

/// The menu bar item: a tray, orange with a count while something waits on you.
struct MenuBarLabel: View {
    let store: Store

    var body: some View {
        HStack(spacing: 2) {
            Image(nsImage: image)
            if store.connected, store.waiting > 0 {
                Text(verbatim: "\(store.waiting)")
            }
        }
    }

    private var image: NSImage {
        if !store.connected {
            return .symbol("tray.and.arrow.down", color: .systemGray)
        }
        if !store.needsYou.isEmpty {
            return .symbol("tray.full.fill", color: .systemOrange)
        }
        if !store.unread.isEmpty {
            return .symbol("tray.full")
        }
        return .symbol("tray")
    }
}

extension NSImage {
    /// An SF Symbol for the menu bar: a template the system tints, or colored.
    static func symbol(_ name: String, color: NSColor? = nil) -> NSImage {
        var config = NSImage.SymbolConfiguration(pointSize: 14, weight: .regular)
        if let color {
            config = config.applying(.init(paletteColors: [color]))
        }
        let image = NSImage(systemSymbolName: name, accessibilityDescription: "Rex Inbox")?
            .withSymbolConfiguration(config) ?? NSImage()
        image.isTemplate = color == nil
        return image
    }
}
