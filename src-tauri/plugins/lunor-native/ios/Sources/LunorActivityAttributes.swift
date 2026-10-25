#if os(iOS)
import ActivityKit
import Foundation

/// Compiled into the app (through this package) and into the Live Activity extension: both copies must keep the same
/// name and shape for ActivityKit to hand the state over. Only the tracked facts live here — the session static part
/// is empty, so one activity follows a whole listening session across tracks.
struct LunorActivityAttributes: ActivityAttributes {
  struct ContentState: Codable, Hashable {
    var title: String
    var artist: String
    /// A downscaled JPEG, already sized for the island. Kept tiny because ActivityKit stores the state for the
    /// extension and a large payload is rejected; nil shows the designed placeholder.
    var artwork: Data?
    var durationMs: Double
    var positionMs: Double
    var isPlaying: Bool
    /// Anchor for the progress bar: `positionMs` was true at this instant.
    var updatedAt: Date
    /// The lyric line core shows right now, from the synced-lyrics cache; nil when the track has none.
    var lyric: String?

    var startedAt: Date { updatedAt.addingTimeInterval(-positionMs / 1000) }
    var endsAt: Date { startedAt.addingTimeInterval(max(durationMs, 1) / 1000) }
    var progress: Double {
      guard durationMs > 0 else { return 0 }
      return min(max(positionMs / durationMs, 0), 1)
    }
  }
}
#endif
