#if os(iOS)
import ActivityKit
import UIKit

/// Mirrors core's now-playing facts into one Live Activity per listening session. Core stays the only source of
/// truth: this never changes playback, never touches the network, and never logs titles or artwork.
@MainActor
final class LunorLiveActivityManager {
  static let shared = LunorLiveActivityManager()

  private var activity: Activity<LunorActivityAttributes>?
  private var last: LunorActivityAttributes.ContentState?
  private var track = ""
  private var sentPositionMs: Double?
  private var sentAt = Date.distantPast
  private var sentPlaying = false
  private var lastSend = Date.distantPast
  private var retryAfter = Date.distantPast
  private var requesting = false
  private var artworkUrl: String?
  private var artwork: Data?

  /// Called on every `nowPlaying` push (~4 Hz). Updates only when something the island shows actually changed, because
  /// ActivityKit budgets updates and the progress bar advances by itself from `updatedAt`.
  func apply(title: String, artist: String, artworkUrl: String?, durationMs: Double, positionMs: Double, playing: Bool) {
    guard Date() >= retryAfter, ActivityAuthorizationInfo().areActivitiesEnabled else { return }
    if self.artworkUrl != artworkUrl {
      self.artworkUrl = artworkUrl
      artwork = nil
    }
    adoptIfNeeded()
    // A track that is only loaded does not get an island: the activity belongs to playback. Once one exists, paused
    // updates keep flowing so the card freezes at the right position.
    guard playing || activity != nil else { return }

    let now = Date()
    let track = "\(title)|\(artist)|\(Int(durationMs.rounded()))"
    let changed = track != self.track || sentPlaying != playing || activity == nil
    let predicted = sentPositionMs.map { $0 + (sentPlaying ? now.timeIntervalSince(sentAt) : 0) * 1000 }
    let jumped = predicted.map { abs(positionMs - $0) > 2000 } ?? false
    guard changed || jumped else { return }
    // A seek can arrive as a burst of pushes; keep one update per second unless the track or state changed.
    guard changed || now.timeIntervalSince(lastSend) >= 1 else { return }

    self.track = track
    send(
      LunorActivityAttributes.ContentState(
        title: title,
        artist: artist,
        artwork: artwork,
        durationMs: max(durationMs, 0),
        positionMs: max(positionMs, 0),
        isPlaying: playing,
        updatedAt: now
      )
    )
  }

  /// The plugin already downloaded the artwork for the lock screen; reuse that image instead of fetching it twice.
  func setArtwork(_ image: UIImage, for url: String) {
    guard url == artworkUrl else { return }
    guard let jpeg = Self.thumbnail(image) else {
      NSLog("[live activity] artwork dropped: still over the size budget")
      return
    }
    guard jpeg != artwork else { return }
    artwork = jpeg
    guard var state = last else { return }
    let elapsed = state.isPlaying ? Date().timeIntervalSince(state.updatedAt) * 1000 : 0
    state.positionMs = min(state.positionMs + elapsed, max(state.durationMs, state.positionMs + elapsed))
    state.artwork = jpeg
    state.updatedAt = Date()
    send(state)
  }

  func stop() {
    let running = activity
    activity = nil
    last = nil
    artworkUrl = nil
    artwork = nil
    track = ""
    sentPositionMs = nil
    sentPlaying = false
    lastSend = .distantPast
    guard let running else { return }
    Task { await running.end(nil, dismissalPolicy: .immediate) }
  }

  private func adoptIfNeeded() {
    guard activity == nil else { return }
    guard let existing = Activity<LunorActivityAttributes>.activities.first(where: { $0.activityState == .active }) else {
      return
    }
    activity = existing
    last = existing.content.state
    sentPositionMs = existing.content.state.positionMs
    sentAt = existing.content.state.updatedAt
    sentPlaying = existing.content.state.isPlaying
  }

  private func send(_ state: LunorActivityAttributes.ContentState) {
    // A request is asynchronous; without this a second poke during the round trip would start a duplicate activity.
    guard !requesting else { return }
    last = state
    sentPositionMs = state.positionMs
    sentAt = state.updatedAt
    sentPlaying = state.isPlaying
    lastSend = Date()
    let content = ActivityContent(state: state, staleDate: Date().addingTimeInterval(8 * 3600))
    if let activity, activity.activityState == .active {
      Task { await activity.update(content) }
      return
    }
    activity = nil
    requesting = true
    Task {
      defer { requesting = false }
      do {
        let requested = try Activity.request(attributes: LunorActivityAttributes(), content: content, pushType: nil)
        // `stop` cleared `last` while the request was in flight: the session is over, do not resurrect it.
        if last == nil {
          await requested.end(nil, dismissalPolicy: .immediate)
        } else {
          activity = requested
        }
      } catch {
        NSLog("[live activity] request failed: %@", error.localizedDescription)
        retryAfter = Date().addingTimeInterval(30)
      }
    }
  }

  /// Two attempts keep the state small enough to travel with the activity; the placeholder is the fallback.
  private static func thumbnail(_ image: UIImage) -> Data? {
    let attempts: [(CGFloat, CGFloat)] = [(96, 0.7), (64, 0.5)]
    for (side, quality) in attempts {
      let scale = min(1, side / max(image.size.width, image.size.height))
      let size = CGSize(
        width: max(1, (image.size.width * scale).rounded()),
        height: max(1, (image.size.height * scale).rounded())
      )
      let format = UIGraphicsImageRendererFormat.default()
      format.scale = 1
      format.opaque = true
      let scaled = UIGraphicsImageRenderer(size: size, format: format).image { _ in
        image.draw(in: CGRect(origin: .zero, size: size))
      }
      if let data = scaled.jpegData(compressionQuality: quality), data.count <= 2500 { return data }
    }
    return nil
  }
}
#endif
