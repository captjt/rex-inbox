import AppKit
import SwiftUI

/// A review fetched for the preview window.
struct Previewed: Equatable {
    let alert: Alert
    let text: String
}

/// A review rex-reviewer has ready, as it would post, with its answers
/// beside it, so you can read it before you post it.
struct PreviewWindow: View {
    let store: Store
    @Environment(\.dismissWindow) private var dismissWindow

    var body: some View {
        Group {
            if let p = store.previewed {
                content(p)
            } else {
                EmptyState(symbol: "doc.text", title: "No review", detail: "Choose Preview on a review that's ready to post.")
            }
        }
        .frame(minWidth: 560, minHeight: 420)
        .onAppear { NSApp.setActivationPolicy(.regular) }
        .onDisappear { NSApp.setActivationPolicy(.accessory) }
    }

    private func content(_ p: Previewed) -> some View {
        // The item as it is now: gone once it's posted, dismissed, or over.
        let live = store.alert(p.alert.id)
        let busy = store.busy.contains(p.alert.id)
        return VStack(alignment: .leading, spacing: 0) {
            VStack(alignment: .leading, spacing: 4) {
                Text(p.alert.title ?? "Review").font(.title3.weight(.semibold))
                Text(p.alert.msg ?? "").font(.callout).foregroundStyle(.secondary).lineLimit(3)
            }
            .padding(16)
            Divider()
            ScrollView {
                Text(Self.render(p.text))
                    .font(.system(.body))
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(16)
            }
            Divider()
            HStack(spacing: 8) {
                if busy { ProgressView().controlSize(.small) }
                if live == nil {
                    Text("This review no longer waits for you.").font(.callout).foregroundStyle(.secondary)
                }
                Spacer()
                Button("Close") { dismissWindow(id: "preview") }
                    .keyboardShortcut(.cancelAction)
                ForEach((live ?? p.alert).decisions.reversed(), id: \.action) { a in
                    if a.action == "approve" {
                        Button(a.label) { answer(a, p) }
                            .buttonStyle(.borderedProminent)
                            .tint(.green)
                    } else {
                        Button(a.label, role: .destructive) { answer(a, p) }
                    }
                }
            }
            .disabled(live == nil || busy)
            .padding(12)
            Banners(store: store)
        }
    }

    private func answer(_ a: Answer, _ p: Previewed) {
        Task {
            await store.answer(a.action, p.alert.id, seen: p.alert.msg)
            if store.problem == nil { dismissWindow(id: "preview") }
        }
    }

    /// Markdown's inline styles (bold, code, links), keeping its lines.
    static func render(_ text: String) -> AttributedString {
        let options = AttributedString.MarkdownParsingOptions(interpretedSyntax: .inlineOnlyPreservingWhitespace)
        return (try? AttributedString(markdown: text, options: options)) ?? AttributedString(text)
    }
}
