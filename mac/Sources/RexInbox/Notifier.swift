import Foundation
import UserNotifications

/// Notifications for what needs you, with the answer in the notification:
/// Approve or Deny a Claude Code permission prompt, Post or Dismiss a review
/// rex-reviewer has ready, Reply to a message, or Open the pane. While the
/// app runs, the hub's own notifications stay quiet.
@MainActor
final class Notifier: NSObject, UNUserNotificationCenterDelegate {
    private weak var store: Store?
    /// How long an alert must stay open before it notifies, so a prompt you
    /// answer right away (or that resolves on its own) never pings you.
    private let grace: Duration = .seconds(2)

    /// UserNotifications needs a real app bundle; `swift run` has none.
    private var available: Bool { Bundle.main.bundleIdentifier != nil }

    private var categories: Set<UNNotificationCategory> = []
    private let openAction = UNNotificationAction(identifier: "open", title: "Open", options: [.foreground])

    /// A category for a set of answers, named by them, so a notification's
    /// buttons say what the hub says (Approve, or Post).
    private func category(_ answers: [Answer]) -> UNNotificationCategory {
        let id = "answers:" + answers.map { "\($0.action)=\($0.label)" }.joined(separator: ",")
        let actions = answers.map {
            UNNotificationAction(identifier: $0.action, title: $0.label, options: $0.action == "deny" ? [.destructive] : [])
        }
        return UNNotificationCategory(identifier: id, actions: actions + [openAction], intentIdentifiers: [])
    }

    func start(store: Store) {
        self.store = store
        guard available else { return }
        let center = UNUserNotificationCenter.current()
        center.delegate = self
        let reply = UNTextInputNotificationAction(
            identifier: "reply", title: "Reply", options: [],
            textInputButtonTitle: "Send", textInputPlaceholder: "Your answer")
        // The answers the hub offers today, registered up front so the first
        // notification has its buttons. Others are added as they turn up.
        categories = [
            category([Answer(action: "approve", label: "Approve"), Answer(action: "deny", label: "Deny")]),
            category([Answer(action: "approve", label: "Post"), Answer(action: "deny", label: "Dismiss")]),
            UNNotificationCategory(identifier: "alert", actions: [openAction], intentIdentifiers: []),
            UNNotificationCategory(identifier: "message", actions: [reply, openAction], intentIdentifiers: []),
        ]
        center.setNotificationCategories(categories)
        center.requestAuthorization(options: [.alert, .sound]) { _, _ in }
    }

    func consider(_ event: HubEvent) {
        guard available else { return }
        switch event.type {
        case "alert_opened":
            guard let a = event.alert, a.category == "needs_you" || a.category == "failed" else { return }
            Task {
                try? await Task.sleep(for: grace)
                guard let store, let now = store.alert(a.id), now.status == "open" else { return }
                post(alert: now)
            }
        case "message_new":
            guard let m = event.message, m.toBlock == nil else { return }
            post(message: m)
        case "alert_resolved":
            if let a = event.alert { withdraw(a.id) }
        case "alert_updated":
            // Answered, dismissed or snoozed: it doesn't need you now.
            if let a = event.alert, a.status != "open" { withdraw(a.id) }
        case "message_updated":
            if let m = event.message, m.status == "done" || m.status == "read" { withdraw(m.id) }
        default:
            break
        }
    }

    private func post(alert a: Alert) {
        let content = UNMutableNotificationContent()
        content.title = a.headline
        content.subtitle = "#\(a.id) · \(a.place)"
        content.body = a.what.isEmpty ? Format.tilde(a.cwd) : a.what
        content.sound = .default
        content.threadIdentifier = a.blockId
        content.categoryIdentifier = a.canAnswer ? categoryID(a.decisions) : "alert"
        content.userInfo = ["id": a.id, "msg": a.msg ?? ""]
        deliver(a.id, content)
    }

    private func categoryID(_ answers: [Answer]) -> String {
        let c = category(answers)
        if !categories.contains(where: { $0.identifier == c.identifier }) {
            categories.insert(c)
            UNUserNotificationCenter.current().setNotificationCategories(categories)
        }
        return c.identifier
    }

    private func post(message m: Message) {
        let content = UNMutableNotificationContent()
        content.title = "Message from \(m.from)"
        content.subtitle = "#\(m.id)" + (m.subject.map { " · \($0)" } ?? "")
        content.body = m.body
        content.sound = .default
        content.threadIdentifier = "thread-\(m.threadId)"
        content.categoryIdentifier = "message"
        content.userInfo = ["id": m.id]
        deliver(m.id, content)
    }

    private func deliver(_ id: Int, _ content: UNNotificationContent) {
        let request = UNNotificationRequest(identifier: "item-\(id)", content: content, trigger: nil)
        UNUserNotificationCenter.current().add(request)
    }

    /// Take back a notification once its item no longer needs you.
    private func withdraw(_ id: Int) {
        let center = UNUserNotificationCenter.current()
        center.removeDeliveredNotifications(withIdentifiers: ["item-\(id)"])
        center.removePendingNotificationRequests(withIdentifiers: ["item-\(id)"])
    }

    // MARK: - UNUserNotificationCenterDelegate

    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter, willPresent notification: UNNotification,
        withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void
    ) {
        completionHandler([.banner, .list, .sound])
    }

    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse,
        withCompletionHandler completionHandler: @escaping () -> Void
    ) {
        let info = response.notification.request.content.userInfo
        let id = info["id"] as? Int ?? 0
        let seen = info["msg"] as? String
        let action = response.actionIdentifier
        let text = (response as? UNTextInputNotificationResponse)?.userText
        Task { @MainActor in
            if let store = self.store {
                switch action {
                case "approve": await store.approve(id, seen: seen)
                case "deny": await store.deny(id, seen: seen)
                case "reply":
                    if let text, !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                        await store.reply(id, text)
                    }
                case UNNotificationDismissActionIdentifier: break
                default: await store.open(id)
                }
            }
            completionHandler()
        }
    }
}
