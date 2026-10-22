import ActivityKit
import Foundation
import HerdrRemoteCore

/// Collects the push tokens ActivityKit hands the app and sends them to the
/// paired computers (`POST /api/apns`): the push-to-start token, which lets
/// a computer start an activity while the app is closed, goes to every
/// computer; each activity's own token goes to the computer it belongs to.
@MainActor
@Observable
final class ActivityTokens {
    struct Running: Identifiable, Equatable {
        let id: String
        let attributes: ChatActivityFacts
        var state: ChatActivityState
        var token: String?
    }

    private(set) var pushToStart: String?
    private(set) var running: [String: Running] = [:]
    private(set) var lastError: String?

    /// The paired computers, kept current by the app model.
    var computers: [Computer] = []
    private let sessions: Sessions
    private var started = false

    init(sessions: Sessions) {
        self.sessions = sessions
    }

    /// Starts watching ActivityKit; safe to call more than once.
    func start() {
        guard !started else { return }
        started = true
        Task {
            for await data in Activity<ChatActivityAttributes>.pushToStartTokenUpdates {
                pushToStart = data.hexString
                await sync()
            }
        }
        Task {
            for await activity in Activity<ChatActivityAttributes>.activityUpdates {
                watch(activity)
            }
        }
        for activity in Activity<ChatActivityAttributes>.activities {
            watch(activity)
        }
    }

    private func watch(_ activity: Activity<ChatActivityAttributes>) {
        guard running[activity.id] == nil else { return }
        running[activity.id] = Running(
            id: activity.id,
            attributes: activity.attributes.facts,
            state: activity.content.state,
            token: activity.pushToken?.hexString
        )
        Task {
            for await data in activity.pushTokenUpdates {
                running[activity.id]?.token = data.hexString
                await sync(computer: activity.attributes.computer)
            }
        }
        Task {
            for await content in activity.contentUpdates {
                running[activity.id]?.state = content.state
            }
        }
        Task {
            for await state in activity.activityStateUpdates where state == .ended || state == .dismissed {
                running[activity.id] = nil
                await sync(computer: activity.attributes.computer)
                return
            }
        }
    }

    /// Sends each paired computer (or only the one named) its tokens.
    func sync(computer name: String? = nil) async {
        for computer in computers where name == nil || computer.name == name {
            let activities = running.values
                .filter { $0.attributes.computer == computer.name }
                .compactMap { running in
                    running.token.map { API.APNsRegistration.Activity(pane: running.attributes.pane, token: $0) }
                }
            let registration = API.APNsRegistration(pushToStart: pushToStart, activities: activities)
            do {
                let session = try await sessions.session(for: computer)
                try await session.client.registerAPNs(registration, bearer: session.token)
                lastError = nil
            } catch {
                await sessions.forget(computer)
                lastError = "\(computer.name): \(error.localizedDescription)"
            }
        }
    }
}
