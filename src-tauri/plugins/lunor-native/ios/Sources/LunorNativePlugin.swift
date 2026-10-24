import AVFoundation
import MediaPlayer
import SwiftRs
import Tauri
import UIKit
import WebKit

// Audio output and lock-screen integration for iOS. Core owns playback state; this class only reports what AVPlayer
// does. Lock-screen / headphone commands are NOT applied here: they are queued as `remote` events and Rust sends them
// back through the same core path as the in-app buttons (a track is PLAYING only after the player confirms it).

class LoadArgs: Decodable { let url: String }
class SeekArgs: Decodable { let positionMs: Double }
class VolumeArgs: Decodable { let volume: Double }
class NowPlayingArgs: Decodable {
  let title: String
  let artist: String
  let artworkUrl: String?
  let durationMs: Double
  let positionMs: Double
  let playing: Bool
}


// MARK: Google sign-in for YouTube (full-screen web view)

/// Same host allowlist as the desktop sign-in window (`login_navigation` in src/commands/youtube.rs). Keep them in step.
private func loginNavigationAllowed(_ url: URL) -> Bool {
  if url.absoluteString == "about:blank" { return true }
  guard url.scheme == "https", url.port == nil, url.user == nil, url.password == nil, let host = url.host else { return false }
  if ["www.youtube.com", "music.youtube.com", "youtube.com",
      "accounts.google.com", "accounts.youtube.com", "consent.google.com", "consent.youtube.com", "myaccount.google.com"].contains(host) {
    return true
  }
  // accounts.google.<cc>, accounts.google.co.<cc>, accounts.google.com.<cc> with a two-letter country code
  guard host.hasPrefix("accounts.google.") else { return false }
  let rest = host.dropFirst("accounts.google.".count).split(separator: ".").map(String.init)
  let code = { (c: String) in c.count == 2 && c.allSatisfy { $0 >= "a" && $0 <= "z" } }
  switch rest.count {
  case 1: return code(rest[0])
  case 2: return (rest[0] == "co" || rest[0] == "com") && code(rest[1])
  default: return false
  }
}

private func isYouTubeOrigin(_ url: URL?) -> Bool {
  guard let url = url, url.scheme == "https", url.port == nil, url.user == nil else { return false }
  return ["www.youtube.com", "music.youtube.com", "youtube.com"].contains(url.host ?? "")
}

class SignInController: UIViewController, WKNavigationDelegate, WKUIDelegate {
  var onDone: (([[String: String]]?) -> Void)?
  private let web: WKWebView
  private var polling = false
  private var finished = false
  private static let sessionNames: Set<String> = ["SAPISID", "__Secure-1PAPISID", "__Secure-3PAPISID"]

  init() {
    let config = WKWebViewConfiguration()
    // Nothing persists: the session lives in RAM until the cookies are handed to core, then the store is dropped.
    config.websiteDataStore = .nonPersistent()
    web = WKWebView(frame: .zero, configuration: config)
    super.init(nibName: nil, bundle: nil)
  }

  required init?(coder: NSCoder) { fatalError("not used") }

  override func viewDidLoad() {
    super.viewDidLoad()
    title = "Sign in to YouTube"
    view.backgroundColor = .black
    navigationItem.leftBarButtonItem = UIBarButtonItem(barButtonSystemItem: .cancel, target: self, action: #selector(cancel))
    // The default WKWebView agent has no `Safari/` token, so YouTube Music answers "browser not supported".
    web.customUserAgent = UIDevice.current.userInterfaceIdiom == .pad
      ? "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Safari/605.1.15"
      : "Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Mobile/15E148 Safari/604.1"
    web.navigationDelegate = self
    web.uiDelegate = self
    web.translatesAutoresizingMaskIntoConstraints = false
    view.addSubview(web)
    NSLayoutConstraint.activate([
      web.topAnchor.constraint(equalTo: view.safeAreaLayoutGuide.topAnchor),
      web.bottomAnchor.constraint(equalTo: view.bottomAnchor),
      web.leadingAnchor.constraint(equalTo: view.leadingAnchor),
      web.trailingAnchor.constraint(equalTo: view.trailingAnchor),
    ])
    if let url = URL(string: "https://accounts.google.com/ServiceLogin?service=youtube&continue=https%3A%2F%2Fmusic.youtube.com%2F") {
      web.load(URLRequest(url: url))
    }
  }

  @objc private func cancel() { finish(nil) }

  private func finish(_ cookies: [[String: String]]?) {
    if finished { return }
    finished = true
    web.stopLoading()
    dismiss(animated: true) { self.onDone?(cookies) }
  }

  func webView(_ webView: WKWebView, decidePolicyFor action: WKNavigationAction, decisionHandler: @escaping (WKNavigationActionPolicy) -> Void) {
    if let url = action.request.url, loginNavigationAllowed(url) { decisionHandler(.allow) } else { decisionHandler(.cancel) }
  }

  // Popups are denied.
  func webView(_ webView: WKWebView, createWebViewWith configuration: WKWebViewConfiguration, for action: WKNavigationAction, windowFeatures: WKWindowFeatures) -> WKWebView? { nil }

  func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
    guard isYouTubeOrigin(webView.url), !polling else { return }
    polling = true
    poll(attempt: 0)
  }

  /// The session cookies land a moment after the first YouTube Music load, and that page fires no further load events.
  private func poll(attempt: Int) {
    if finished { return }
    web.configuration.websiteDataStore.httpCookieStore.getAllCookies { [weak self] cookies in
      guard let self = self, !self.finished else { return }
      let relevant = cookies.filter { $0.domain.hasSuffix("youtube.com") || $0.domain.hasSuffix("google.com") }
      if relevant.contains(where: { SignInController.sessionNames.contains($0.name) }) {
        self.finish(relevant.map { ["name": $0.name, "value": $0.value, "domain": $0.domain] })
      } else if attempt < 9 {
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.5) { self.poll(attempt: attempt + 1) }
      } else {
        self.polling = false
      }
    }
  }
}

class LunorNativePlugin: Plugin {
  private let player = AVPlayer()
  private var item: AVPlayerItem?
  private var itemObservation: NSKeyValueObservation?
  private var controlObservation: NSKeyValueObservation?
  private var pendingLoad: Invoke?
  private var pendingPlay: Invoke?
  private var ended = false
  private var failed = false
  private var remote: [[String: Any]] = []
  private var artworkUrl: String?
  private var artwork: MPMediaItemArtwork?
  private let lock = NSLock()

  override func load(webview: WKWebView) {
    player.automaticallyWaitsToMinimizeStalling = true
    controlObservation = player.observe(\.timeControlStatus, options: [.new]) { [weak self] player, _ in
      guard let self = self, player.timeControlStatus == .playing else { return }
      DispatchQueue.main.async {
        self.pendingPlay?.resolve()
        self.pendingPlay = nil
      }
    }
    let center = NotificationCenter.default
    center.addObserver(self, selector: #selector(didEnd(_:)), name: .AVPlayerItemDidPlayToEndTime, object: nil)
    center.addObserver(self, selector: #selector(didFail(_:)), name: .AVPlayerItemFailedToPlayToEndTime, object: nil)
    center.addObserver(self, selector: #selector(interruption(_:)), name: AVAudioSession.interruptionNotification, object: nil)
    center.addObserver(self, selector: #selector(routeChange(_:)), name: AVAudioSession.routeChangeNotification, object: nil)
    registerRemoteCommands()
  }

  // MARK: commands

  @objc public func load(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(LoadArgs.self)
    guard let url = URL(string: args.url),
      url.isFileURL || url.host == "127.0.0.1" || (url.scheme == "https" && url.host.flatMap { $0.hasSuffix(".googlevideo.com") } == true)
    else {
      invoke.reject("url not allowed")
      return
    }
    DispatchQueue.main.async {
      self.cancelPending("superseded")
      let session = AVAudioSession.sharedInstance()
      do {
        try session.setCategory(.playback, mode: .default)
        try session.setActive(true)
      } catch {
        invoke.reject("audio session: \(error.localizedDescription)")
        return
      }
      self.ended = false
      self.failed = false
      // The cache names files `*.audio`; AVPlayer cannot sniff the container from that, so say it (AAC in MP4 is
      // the only format core hands to iOS).
      let asset = AVURLAsset(url: url, options: ["AVURLAssetOutOfBandMIMETypeKey": "audio/mp4"])
      let item = AVPlayerItem(asset: asset)
      self.item = item
      self.pendingLoad = invoke
      self.itemObservation = item.observe(\.status, options: [.new]) { [weak self] item, _ in
        DispatchQueue.main.async {
          guard let self = self, self.item === item else { return }
          switch item.status {
          case .readyToPlay:
            self.pendingLoad?.resolve()
            self.pendingLoad = nil
          case .failed:
            self.failed = true
            let reason = item.error?.localizedDescription ?? asset.error?.localizedDescription ?? "unknown"
            self.pendingLoad?.reject("load failed: \(reason)")
            self.pendingLoad = nil
          default: break
          }
        }
      }
      self.player.replaceCurrentItem(with: item)
      self.player.pause()
    }
  }

  @objc public func play(_ invoke: Invoke) throws {
    DispatchQueue.main.async {
      guard self.item != nil, !self.failed else {
        invoke.reject("nothing loaded")
        return
      }
      if self.player.timeControlStatus == .playing {
        invoke.resolve()
        return
      }
      self.pendingPlay?.reject("superseded")
      self.pendingPlay = invoke
      self.player.play()
    }
  }

  @objc public func pause(_ invoke: Invoke) throws {
    DispatchQueue.main.async {
      self.pendingPlay?.reject("superseded")
      self.pendingPlay = nil
      self.player.pause()
      invoke.resolve()
    }
  }

  @objc public func stop(_ invoke: Invoke) throws {
    DispatchQueue.main.async {
      self.cancelPending("stopped")
      self.player.pause()
      self.player.replaceCurrentItem(with: nil)
      self.itemObservation = nil
      self.item = nil
      self.ended = false
      self.failed = false
      MPNowPlayingInfoCenter.default().nowPlayingInfo = nil
      invoke.resolve()
    }
  }

  @objc public func seek(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(SeekArgs.self)
    DispatchQueue.main.async {
      guard self.item != nil else {
        invoke.reject("nothing loaded")
        return
      }
      let target = CMTime(seconds: max(0, args.positionMs) / 1000.0, preferredTimescale: 1000)
      self.player.seek(to: target, toleranceBefore: .zero, toleranceAfter: .zero) { finished in
        finished ? invoke.resolve() : invoke.reject("seek interrupted")
      }
    }
  }

  @objc public func setVolume(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(VolumeArgs.self)
    DispatchQueue.main.async {
      self.player.volume = Float(min(max(args.volume, 0), 100) / 100.0)
      invoke.resolve()
    }
  }

  /// Latest facts, read-only. `remote` is drained so each lock-screen press is delivered once.
  @objc public func poll(_ invoke: Invoke) throws {
    DispatchQueue.main.async {
      var status = "idle"
      if self.item != nil {
        if self.failed {
          status = "failed"
        } else if self.ended {
          status = "ended"
        } else if self.item?.status != .readyToPlay {
          status = "loading"
        } else {
          switch self.player.timeControlStatus {
          case .playing: status = "playing"
          case .waitingToPlayAtSpecifiedRate: status = self.pendingPlay != nil || self.player.rate > 0 ? "buffering" : "paused"
          default: status = "paused"
          }
        }
      }
      let position = self.player.currentTime().seconds
      let duration = self.item?.duration.seconds ?? 0
      self.lock.lock()
      let events = self.remote
      self.remote = []
      self.lock.unlock()
      invoke.resolve([
        "status": status,
        "positionMs": position.isFinite ? max(0, position * 1000) : 0,
        "durationMs": duration.isFinite ? max(0, duration * 1000) : 0,
        "remote": events,
      ])
    }
  }

  @objc public func nowPlaying(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(NowPlayingArgs.self)
    DispatchQueue.main.async {
      var info: [String: Any] = [
        MPMediaItemPropertyTitle: args.title,
        MPMediaItemPropertyArtist: args.artist,
        MPMediaItemPropertyPlaybackDuration: args.durationMs / 1000.0,
        MPNowPlayingInfoPropertyElapsedPlaybackTime: args.positionMs / 1000.0,
        MPNowPlayingInfoPropertyPlaybackRate: args.playing ? 1.0 : 0.0,
        MPNowPlayingInfoPropertyDefaultPlaybackRate: 1.0,
      ]
      if let art = self.artwork, self.artworkUrl == args.artworkUrl { info[MPMediaItemPropertyArtwork] = art }
      MPNowPlayingInfoCenter.default().nowPlayingInfo = info
      MPNowPlayingInfoCenter.default().playbackState = args.playing ? .playing : .paused
      if let raw = args.artworkUrl, raw != self.artworkUrl, let url = URL(string: raw), url.scheme == "https" {
        self.artworkUrl = raw
        self.artwork = nil
        URLSession.shared.dataTask(with: url) { data, _, _ in
          guard let data = data, let image = UIImage(data: data) else { return }
          let art = MPMediaItemArtwork(boundsSize: image.size) { _ in image }
          DispatchQueue.main.async {
            guard self.artworkUrl == raw else { return }
            self.artwork = art
            var next = MPNowPlayingInfoCenter.default().nowPlayingInfo ?? [:]
            next[MPMediaItemPropertyArtwork] = art
            MPNowPlayingInfoCenter.default().nowPlayingInfo = next
          }
        }.resume()
      }
      invoke.resolve()
    }
  }


  /// Full-screen Google sign-in. Resolves with the YouTube / Google cookies once the user is signed in, or an empty list if
  /// the user cancels.
  /// Core validates and stores them; this side keeps nothing.
  @objc public func signIn(_ invoke: Invoke) throws {
    DispatchQueue.main.async {
      guard let presenter = LunorNativePlugin.topViewController() else {
        invoke.reject("no view controller")
        return
      }
      let controller = SignInController()
      controller.onDone = { cookies in
        // Cancelling is a normal outcome, reported as an empty list; reject is for real failures.
        invoke.resolve(["cookies": cookies ?? []])
      }
      let nav = UINavigationController(rootViewController: controller)
      nav.modalPresentationStyle = .fullScreen
      presenter.present(nav, animated: true)
    }
  }

  private static func topViewController() -> UIViewController? {
    let scene = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }.first { $0.activationState == .foregroundActive }
    var top = scene?.windows.first(where: { $0.isKeyWindow })?.rootViewController
    while let next = top?.presentedViewController { top = next }
    return top
  }

  // MARK: notifications

  @objc private func didEnd(_ note: Notification) {
    DispatchQueue.main.async { if (note.object as? AVPlayerItem) === self.item { self.ended = true } }
  }

  @objc private func didFail(_ note: Notification) {
    DispatchQueue.main.async { if (note.object as? AVPlayerItem) === self.item { self.failed = true } }
  }

  @objc private func interruption(_ note: Notification) {
    guard let raw = note.userInfo?[AVAudioSessionInterruptionTypeKey] as? UInt,
      AVAudioSession.InterruptionType(rawValue: raw) == .began
    else { return }
    push(["kind": "pause"])
  }

  @objc private func routeChange(_ note: Notification) {
    guard let raw = note.userInfo?[AVAudioSessionRouteChangeReasonKey] as? UInt,
      AVAudioSession.RouteChangeReason(rawValue: raw) == .oldDeviceUnavailable
    else { return }
    push(["kind": "pause"])
  }

  // MARK: remote commands

  private func push(_ event: [String: Any]) {
    lock.lock()
    if remote.count < 32 { remote.append(event) }
    lock.unlock()
  }

  private func registerRemoteCommands() {
    let center = MPRemoteCommandCenter.shared()
    let simple: [(MPRemoteCommand, String)] = [
      (center.playCommand, "play"),
      (center.pauseCommand, "pause"),
      (center.togglePlayPauseCommand, "toggle"),
      (center.nextTrackCommand, "next"),
      (center.previousTrackCommand, "previous"),
    ]
    for (command, kind) in simple {
      command.isEnabled = true
      command.addTarget { [weak self] _ in
        self?.push(["kind": kind])
        return .success
      }
    }
    center.changePlaybackPositionCommand.isEnabled = true
    center.changePlaybackPositionCommand.addTarget { [weak self] event in
      guard let event = event as? MPChangePlaybackPositionCommandEvent else { return .commandFailed }
      self?.push(["kind": "seek", "positionMs": event.positionTime * 1000])
      return .success
    }
  }

  private func cancelPending(_ reason: String) {
    pendingLoad?.reject(reason)
    pendingLoad = nil
    pendingPlay?.reject(reason)
    pendingPlay = nil
  }
}

@_cdecl("init_plugin_lunor_native")
public func initPlugin() -> Plugin {
  return LunorNativePlugin()
}
