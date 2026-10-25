#if os(iOS)
import Foundation

/// Transport presses from the Dynamic Island / lock screen. The intent runs in the app's process, possibly before the
/// Tauri plugin has loaded, so the press waits in standard defaults until `poll` drains it. The intents write here
/// under the same key (see `LunorPlaybackIntents.swift`); keep the two in step.
enum LunorRemoteQueue {
  private static let key = "io.lunorx.mobile.liveActivity.remote"
  private static let kinds: Set<String> = ["previous", "toggle", "next"]

  static func drain() -> [[String: Any]] {
    let pending = UserDefaults.standard.stringArray(forKey: key) ?? []
    guard !pending.isEmpty else { return [] }
    UserDefaults.standard.removeObject(forKey: key)
    return pending.filter(kinds.contains).map { ["kind": $0] }
  }
}
#endif
