#if os(iOS)
import AppIntents
import Foundation

/// Transport buttons on the Live Activity run in the app process. They never touch AVPlayer: each one only appends its
/// action to a small UserDefaults queue that `LunorRemoteQueue` (plugin) prepends to `poll`, so Rust stays the source of
/// truth and the same rules apply as for lock-screen and headphone presses.
enum LunorLiveActivityAction: String, CaseIterable {
  case previous
  case toggle
  case next

  /// Shared with `LunorRemoteQueue` in the plugin; the extension has no other way to reach the app.
  static let queueKey = "io.lunorx.mobile.liveActivity.remote"

  func enqueue() {
    let defaults = UserDefaults.standard
    var pending = defaults.stringArray(forKey: Self.queueKey) ?? []
    if pending.count >= 32 { pending.removeFirst() }
    pending.append(rawValue)
    defaults.set(pending, forKey: Self.queueKey)
  }
}

struct LunorPreviousTrackIntent: LiveActivityIntent {
  static var title: LocalizedStringResource = "Previous Track"
  static var isDiscoverable = false

  func perform() async throws -> some IntentResult {
    LunorLiveActivityAction.previous.enqueue()
    return .result()
  }
}

struct LunorTogglePlaybackIntent: LiveActivityIntent {
  static var title: LocalizedStringResource = "Play or Pause"
  static var isDiscoverable = false

  func perform() async throws -> some IntentResult {
    LunorLiveActivityAction.toggle.enqueue()
    return .result()
  }
}

struct LunorNextTrackIntent: LiveActivityIntent {
  static var title: LocalizedStringResource = "Next Track"
  static var isDiscoverable = false

  func perform() async throws -> some IntentResult {
    LunorLiveActivityAction.next.enqueue()
    return .result()
  }
}
#endif
