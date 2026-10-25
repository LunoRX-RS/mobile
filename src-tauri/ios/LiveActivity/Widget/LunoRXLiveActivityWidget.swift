#if os(iOS)
import ActivityKit
import SwiftUI
import UIKit
import WidgetKit

/// Lumen Lavender on near-black. One Light Rule: the accent marks live state (progress, playing) and nothing else.
private enum LunorPalette {
  static let lavender = Color(red: 0xC5 / 255, green: 0xB4 / 255, blue: 0xFF / 255)
  static let canvas = Color(red: 0x0B / 255, green: 0x0B / 255, blue: 0x0D / 255)
  static let secondary = Color(red: 0xA3 / 255, green: 0xA8 / 255, blue: 0xB2 / 255)
  static let primary = Color.white
}

@main
struct LunoRXLiveActivityBundle: WidgetBundle {
  var body: some Widget {
    LunoRXLiveActivityWidget()
  }
}

struct LunoRXLiveActivityWidget: Widget {
  var body: some WidgetConfiguration {
    ActivityConfiguration(for: LunorActivityAttributes.self) { context in
      LockScreenCard(state: context.state)
        .activityBackgroundTint(LunorPalette.canvas)
        .activitySystemActionForegroundColor(LunorPalette.lavender)
    } dynamicIsland: { context in
      DynamicIsland {
        DynamicIslandExpandedRegion(.leading) {
          Artwork(state: context.state, side: 64, radius: 12)
            .padding(.leading, 4)
        }
        DynamicIslandExpandedRegion(.center) {
          VStack(alignment: .leading, spacing: 2) {
            Text(context.state.title)
              .font(.system(size: 17, weight: .semibold))
              .foregroundStyle(LunorPalette.primary)
              .lineLimit(1)
            Text(context.state.artist)
              .font(.system(size: 13))
              .foregroundStyle(LunorPalette.secondary)
              .lineLimit(1)
          }
          .frame(maxWidth: .infinity, alignment: .leading)
        }
        DynamicIslandExpandedRegion(.trailing) {
          Indicator(playing: context.state.isPlaying, compact: false)
            .padding(.trailing, 4)
        }
        DynamicIslandExpandedRegion(.bottom) {
          VStack(spacing: 10) {
            Progress(state: context.state)
            Controls(playing: context.state.isPlaying)
          }
          .padding(.horizontal, 4)
        }
      } compactLeading: {
        Artwork(state: context.state, side: 26, radius: 7)
      } compactTrailing: {
        Indicator(playing: context.state.isPlaying, compact: true)
      } minimal: {
        Image(systemName: "waveform")
          .font(.system(size: 16, weight: .semibold))
          .foregroundStyle(LunorPalette.lavender)
      }
      .keylineTint(LunorPalette.lavender)
    }
  }
}

private struct LockScreenCard: View {
  let state: LunorActivityAttributes.ContentState

  var body: some View {
    VStack(spacing: 12) {
      HStack(spacing: 12) {
        Artwork(state: state, side: 52, radius: 10)
        VStack(alignment: .leading, spacing: 3) {
          Text(state.title)
            .font(.system(size: 15, weight: .semibold))
            .foregroundStyle(LunorPalette.primary)
            .lineLimit(1)
          Text(state.artist)
            .font(.system(size: 12))
            .foregroundStyle(LunorPalette.secondary)
            .lineLimit(1)
        }
        Spacer(minLength: 0)
        Controls(playing: state.isPlaying)
      }
      Progress(state: state)
    }
    .padding(.horizontal, 16)
    .padding(.vertical, 14)
  }
}

private struct Artwork: View {
  let state: LunorActivityAttributes.ContentState
  let side: CGFloat
  let radius: CGFloat

  var body: some View {
    Group {
      if let data = state.artwork, let image = UIImage(data: data) {
        Image(uiImage: image)
          .resizable()
          .scaledToFill()
      } else {
        ZStack {
          LinearGradient(
            colors: [LunorPalette.lavender.opacity(0.22), LunorPalette.lavender.opacity(0.04)],
            startPoint: .topLeading,
            endPoint: .bottomTrailing
          )
          Image(systemName: "music.note")
            .font(.system(size: side * 0.34, weight: .semibold))
            .foregroundStyle(LunorPalette.lavender.opacity(0.75))
        }
      }
    }
    .frame(width: side, height: side)
    .clipShape(RoundedRectangle(cornerRadius: radius, style: .continuous))
    .accessibilityHidden(true)
  }
}

/// Static three-bar mark while playing, pause glyph while paused. No animation: the system does not run view animations
/// inside a Live Activity, and a moving equalizer would burn update budget for nothing.
private struct Indicator: View {
  let playing: Bool
  let compact: Bool

  var body: some View {
    Group {
      if playing {
        HStack(alignment: .center, spacing: 2) {
          ForEach([8.0, 14.0, 10.0], id: \.self) { height in
            Capsule()
              .fill(LunorPalette.lavender)
              .frame(width: 2.5, height: height)
          }
        }
        .frame(height: 14)
        .accessibilityLabel("Playing")
      } else {
        Image(systemName: compact ? "pause.fill" : "pause")
          .font(.system(size: compact ? 11 : 14, weight: .semibold))
          .foregroundStyle(LunorPalette.primary.opacity(0.9))
          .accessibilityLabel("Paused")
      }
    }
  }
}

/// Live progress advances by itself from `startedAt` / `endsAt`, so a 4 Hz push rate never means 4 Hz view updates.
private struct Progress: View {
  let state: LunorActivityAttributes.ContentState

  private var interval: ClosedRange<Date> {
    let start = state.startedAt
    let end = max(state.endsAt, start.addingTimeInterval(1))
    return start...end
  }

  var body: some View {
    VStack(spacing: 5) {
      if state.isPlaying {
        ProgressView(timerInterval: interval, countsDown: false) {
          EmptyView()
        } currentValueLabel: {
          EmptyView()
        }
        .progressViewStyle(.linear)
        .tint(LunorPalette.lavender)

        HStack {
          Text(timerInterval: interval, countsDown: false)
          Spacer(minLength: 8)
          Text(timerInterval: interval, countsDown: true)
        }
        .font(.system(size: 11, weight: .medium).monospacedDigit())
        .foregroundStyle(LunorPalette.secondary)
      } else {
        ProgressView(value: state.progress)
          .progressViewStyle(.linear)
          .tint(LunorPalette.lavender)

        HStack {
          Text(Self.clock(state.positionMs))
          Spacer(minLength: 8)
          Text(Self.clock(max(state.durationMs - state.positionMs, 0)))
        }
        .font(.system(size: 11, weight: .medium).monospacedDigit())
        .foregroundStyle(LunorPalette.secondary)
      }
    }
    .accessibilityElement(children: .ignore)
    .accessibilityLabel(state.isPlaying ? "Playing" : "Paused")
    .accessibilityValue("\(Self.clock(state.positionMs)) of \(Self.clock(state.durationMs))")
  }

  private static func clock(_ ms: Double) -> String {
    let total = Int((ms / 1000).rounded(.down))
    let seconds = total % 60
    let minutes = (total / 60) % 60
    let hours = total / 3600
    return hours > 0
      ? String(format: "%d:%02d:%02d", hours, minutes, seconds)
      : String(format: "%d:%02d", minutes, seconds)
  }
}

private struct Controls: View {
  let playing: Bool

  var body: some View {
    HStack(spacing: 26) {
      Button(intent: LunorPreviousTrackIntent()) {
        Image(systemName: "backward.fill")
          .font(.system(size: 17, weight: .semibold))
          .foregroundStyle(LunorPalette.primary.opacity(0.9))
      }
      .accessibilityLabel("Previous track")

      Button(intent: LunorTogglePlaybackIntent()) {
        Image(systemName: playing ? "pause.fill" : "play.fill")
          .font(.system(size: 24, weight: .semibold))
          .foregroundStyle(LunorPalette.lavender)
      }
      .accessibilityLabel(playing ? "Pause" : "Play")

      Button(intent: LunorNextTrackIntent()) {
        Image(systemName: "forward.fill")
          .font(.system(size: 17, weight: .semibold))
          .foregroundStyle(LunorPalette.primary.opacity(0.9))
      }
      .accessibilityLabel("Next track")
    }
    .buttonStyle(.plain)
  }
}
#endif
