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

/// YouTube answers a phone user agent with the mobile site at `m.youtube.com`, and every other `*.youtube.com` host is
/// the same service. The leading dot keeps a lookalike such as `evil-youtube.com` out.
private func youtubeHost(_ host: String) -> Bool {
  host == "youtube.com" || host.hasSuffix(".youtube.com")
}

/// Google runs sign-in across its own host family: `google.com` and subdomains such as accounts, consent and the One
/// Google Bar, plus the account's country domain (`google.<cc>`, `google.co.<cc>`, `google.com.<cc>`) with a two-letter
/// code. A lookalike such as `notgoogle.com` or `google.co.th.evil.test` does not pass.
private func googleHost(_ host: String) -> Bool {
  // Like Rust's split('.') and Kotlin's split("."), keep empty labels so `google..com` and a trailing dot fail.
  let labels = host.components(separatedBy: ".")
  guard let index = labels.firstIndex(of: "google") else { return false }
  let rest = Array(labels[(index + 1)...])
  let code = { (c: String) in c.count == 2 && c.allSatisfy { $0 >= "a" && $0 <= "z" } }
  switch rest.count {
  case 1: return rest[0] == "com" || code(rest[0])
  case 2: return (rest[0] == "co" || rest[0] == "com") && code(rest[1])
  default: return false
  }
}

/// Same host allowlist as the desktop sign-in window (`login_navigation` in src/commands/youtube.rs). Keep them in step.
private func loginNavigationAllowed(_ url: URL) -> Bool {
  if url.absoluteString == "about:blank" { return true }
  guard url.scheme == "https", url.port == nil, url.user == nil, url.password == nil, let host = url.host?.lowercased() else { return false }
  return youtubeHost(host) || googleHost(host)
}

private func isYouTubeOrigin(_ url: URL?) -> Bool {
  guard let url = url, url.scheme == "https", url.port == nil, url.user == nil, let host = url.host?.lowercased() else { return false }
  return youtubeHost(host)
}

/// Host-only trace of the sign-in window: which hop loaded, failed or was blocked. Never a full URL, query or cookie.
/// NSLog is visible in Console.app and `xcrun devicectl device console`; with `LUNORX_SIGNIN_LOG` set the same lines
/// are also appended to `tmp/lunorx-signin.log`, which `xcrun devicectl device copy from --domain-type tmp` reads.
private func signInLog(_ message: String) {
  NSLog("[youtube sign-in] %@", message)
  guard ProcessInfo.processInfo.environment["LUNORX_SIGNIN_LOG"] != nil else { return }
  let path = (NSTemporaryDirectory() as NSString).appendingPathComponent("lunorx-signin.log")
  let line = "[\(Int(Date().timeIntervalSince1970))] \(message)\n"
  guard let data = line.data(using: .utf8) else { return }
  if let handle = FileHandle(forWritingAtPath: path) {
    handle.seekToEndOfFile()
    handle.write(data)
    handle.closeFile()
  } else {
    FileManager.default.createFile(atPath: path, contents: data)
  }
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
    signInLog(cookies == nil ? "closed: cancelled" : "closed: session captured")
    web.stopLoading()
    dismiss(animated: true) { self.onDone?(cookies) }
  }

  func webView(_ webView: WKWebView, decidePolicyFor action: WKNavigationAction, decisionHandler: @escaping (WKNavigationActionPolicy) -> Void) {
    guard let url = action.request.url else {
      signInLog("navigation blocked: no url")
      decisionHandler(.cancel)
      return
    }
    let allowed = loginNavigationAllowed(url)
    if !allowed {
      let frame = action.targetFrame?.isMainFrame == false ? "frame" : "page"
      signInLog("navigation blocked (\(frame)): \(url.scheme ?? "-")://\(url.host ?? "-")")
    }
    decisionHandler(allowed ? .allow : .cancel)
  }

  // Popups are denied.
  func webView(_ webView: WKWebView, createWebViewWith configuration: WKWebViewConfiguration, for action: WKNavigationAction, windowFeatures: WKWindowFeatures) -> WKWebView? {
    signInLog("popup blocked: \(action.request.url?.host ?? "-")")
    return nil
  }

  func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
    signInLog("loaded: \(webView.url?.host ?? "-")")
    guard isYouTubeOrigin(webView.url), !polling else { return }
    polling = true
    poll(attempt: 0)
  }

  func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) {
    let failure = error as NSError
    signInLog("load failed: \(webView.url?.host ?? "-") \(failure.domain) \(failure.code)")
  }

  func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) {
    let failure = error as NSError
    signInLog("load failed: \(webView.url?.host ?? "-") \(failure.domain) \(failure.code)")
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
        signInLog("no session cookies after \(attempt + 1) polls")
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
  private var realDurationMs: Double?
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
      // YouTube's progressive-fragmented m4a carries a placeholder moov, so AVFoundation reads exactly 2x the true
      // duration (measured 8/8 cached streams). The top-level `sidx` box holds the real sum; take it from the file.
      self.realDurationMs = url.isFileURL ? Self.sidxDurationMs(path: url.path) : nil
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
            let reason = item.error?.localizedDescription ?? "unknown"
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
      self.realDurationMs = nil
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
      // The item clock runs 1:1 with wall time but its timeline is the doubled one, so after the true media end
      // AVPlayer just runs silent. Close the track at the `sidx` end instead of ~2x the length.
      if let real = self.realDurationMs, self.player.timeControlStatus == .playing,
        self.player.currentTime().seconds * 1000 >= real
      {
        self.player.pause()
        self.ended = true
      }
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
      var durationMs = (self.item?.duration.seconds ?? 0)
      durationMs = durationMs.isFinite ? max(0, durationMs * 1000) : 0
      // Only trust `sidx` over the asset when the asset clearly exceeds it; a misread then cannot truncate a real track.
      if let real = self.realDurationMs, durationMs > real * 1.05 { durationMs = real }
      self.lock.lock()
      let events = self.remote
      self.remote = []
      self.lock.unlock()
      invoke.resolve([
        "status": status,
        "positionMs": position.isFinite ? max(0, position * 1000) : 0,
        "durationMs": durationMs,
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
      // The system Now Playing card is the only lock-screen / island presentation: it carries artwork, transport and
      // AirPods buttons, driven by MPNowPlayingInfoCenter plus the remote commands registered below.
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

  // MARK: mp4 duration

  /// Sum the segment durations of the first top-level `sidx` box (ISO 14496-12), which for YouTube's m4a streams is
  /// the true media length. Returns milliseconds, or nil when the box is absent or unparsable (then the asset value
  /// is used unchanged).
  private static func sidxDurationMs(path: String) -> Double? {
    guard let handle = FileHandle(forReadingAtPath: path) else { return nil }
    defer { try? handle.close() }
    // `sidx` sits before `mdat` near the front; a small stream can have a long entry table, so read up to 1 MiB.
    let bytes = [UInt8](handle.readData(ofLength: 1 << 20))
    func be32(_ p: Int) -> UInt32? {
      guard p + 4 <= bytes.count else { return nil }
      return UInt32(bytes[p]) << 24 | UInt32(bytes[p + 1]) << 16 | UInt32(bytes[p + 2]) << 8 | UInt32(bytes[p + 3])
    }
    func be16(_ p: Int) -> UInt16? {
      guard p + 2 <= bytes.count else { return nil }
      return UInt16(bytes[p]) << 8 | UInt16(bytes[p + 1])
    }
    var p = 0
    while p + 8 <= bytes.count, let rawSize = be32(p) {
      let size: Int
      let body: Int
      switch rawSize {
      case 0:  // extends to end-of-file: everything past here is inside this box
        size = bytes.count - p
        body = p + 8
      case 1:  // 64-bit largesize
        guard p + 16 <= bytes.count, let high = be32(p + 8), let low = be32(p + 12) else { return nil }
        let wide = UInt64(high) << 32 | UInt64(low)
        guard wide <= UInt64(Int.max) else { return nil }
        size = Int(wide)
        body = p + 16
      default:
        size = Int(rawSize)
        body = p + 8
      }
      guard size >= 8, body <= bytes.count else { return nil }
      let tag = String(bytes: bytes[(p + 4)..<(p + 8)], encoding: .ascii) ?? ""
      if tag == "sidx" {
        // Locate the entry table self-validating: the u16 just before the table equals the number of 12-byte entries
        // that exactly fill the box. ISO says body+18 (v0) / body+26 (v1); YouTube's muxer writes the header 4 bytes
        // longer, so scan the first 64 body bytes rather than assume one offset.
        let boxEnd = p + size
        var table: (start: Int, count: Int)? = nil
        var cand = body + 20
        while cand + 2 <= min(boxEnd, body + 64), table == nil {
          if let n = be16(cand - 2), boxEnd > cand, (boxEnd - cand) % 12 == 0, Int(n) * 12 == boxEnd - cand {
            table = (cand, Int(n))
          }
          cand += 4
        }
        guard let table else { return nil }
        // AAC in MP4 is the only container core hands iOS, so the timescale is the sample rate; find it among the
        // header words instead of trusting one position (YouTube's shifted header puts it at body+8, not body+4).
        var timescale: UInt32? = nil
        var q = body + 4
        while q + 4 <= table.start {
          if let v = be32(q), v == 44100 || v == 48000 {
            timescale = v
            break
          }
          q += 4
        }
        guard let timescale else { return nil }
        var entry = table.start
        var total: UInt64 = 0
        for _ in 0..<table.count {
          guard let segmentDuration = be32(entry + 4) else { return nil }
          total += UInt64(segmentDuration)
          entry += 12
        }
        return Double(total) * 1000.0 / Double(timescale)
      }
      // The next box starts past what was read: the sidx (if any) is beyond, so stop without a value.
      guard p + size <= bytes.count else { return nil }
      p += size
    }
    return nil
  }
}

@_cdecl("init_plugin_lunor_native")
public func initPlugin() -> Plugin {
  return LunorNativePlugin()
}
