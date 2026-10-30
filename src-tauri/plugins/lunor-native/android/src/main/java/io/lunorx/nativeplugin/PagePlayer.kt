package io.lunorx.nativeplugin

import android.app.Activity
import android.net.Uri
import android.content.Context
import android.os.Handler
import android.os.PowerManager
import android.util.Log
import android.view.View
import android.view.ViewGroup
import android.webkit.WebChromeClient
import android.webkit.WebResourceRequest
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.FrameLayout
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject

/**
 * Plays a track through YouTube's own page in a hidden web view, for when the direct stream is refused (the same
 * fallback the desktop app has). The page is the provider's own player, ads included. The view has no JavaScript
 * bridge: the page reports its state through its title and takes commands as plain script calls, so remote content
 * never reaches native code. Every method runs on the main thread. Only the page's own reports settle playback state.
 */
/**
 * A web view that always tells the page it is on screen. When the app goes to the background the window stops being
 * visible and the page's player is paused (and its timers throttled) on the strength of that alone; this view only ever
 * plays audio, so it keeps saying "visible".
 */
private class PageWebView(context: Context) : WebView(context) {
    override fun onWindowVisibilityChanged(visibility: Int) = super.onWindowVisibilityChanged(View.VISIBLE)

    override fun onVisibilityChanged(changedView: View, visibility: Int) = super.onVisibilityChanged(changedView, View.VISIBLE)
}

class PagePlayer(private val activity: Activity, private val main: Handler) {
    private var web: WebView? = null
    private var expected = ""
    private var loading = false
    private var pending: Invoke? = null
    private var deadline: Runnable? = null
    private var state = "waiting"
    // Core has not asked for sound yet: whatever the page does by itself (it autoplays) is held paused and reported so.
    private var wanted = false
    private var positionMs = 0L
    private var durationMs = 0L

    // The media session shows this while the page holds the track; see [PageMirror].
    private val mirror = PageMirror()
    private var shown = false

    // Keeps the CPU awake while the page plays with the screen off; released on pause, end, error and destroy.
    private var lock: PowerManager.WakeLock? = null

    private fun holdAwake(on: Boolean) {
        if (on && lock?.isHeld != true) {
            val power = activity.getSystemService(Context.POWER_SERVICE) as PowerManager
            lock = power.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "lunorx:page").apply {
                setReferenceCounted(false)
                acquire(WAKE_LIMIT_MS)
            }
        } else if (!on) {
            lock?.takeIf { it.isHeld }?.release()
            lock = null
        }
    }

    val active: Boolean get() = web != null

    /** Title, artist and artwork for the system media card; the page's own title is not for display. */
    fun describe(title: String?, artist: String?, artworkUrl: String?) = mirror.describe(title, artist, artworkUrl)

    private fun refresh() {
        val live = wanted && state != "ended" && state != "error"
        mirror.update(live, live && state == "waiting", state == "ended", positionMs, durationMs)
    }

    fun load(videoId: String, invoke: Invoke) {
        destroy()
        expected = videoId
        loading = true
        state = "waiting"
        wanted = false
        positionMs = 0
        durationMs = 0
        pending = invoke
        val view = PageWebView(activity)
        with(view.settings) {
            javaScriptEnabled = true
            domStorageEnabled = true
            allowFileAccess = false
            allowContentAccess = false
            setSupportMultipleWindows(false)
            javaScriptCanOpenWindowsAutomatically = false
            mediaPlaybackRequiresUserGesture = false
            // The default WebView agent carries "; wv", which Google serves a different page for.
            userAgentString = userAgentString.replace("; wv", "")
        }
        view.webViewClient = object : WebViewClient() {
            // `www.youtube.com` answers a phone with `m.youtube.com`; the sign-in rules already cover both and Google.
            override fun shouldOverrideUrlLoading(v: WebView, request: WebResourceRequest): Boolean =
                !SignInHosts.allowed(request.url)

            override fun onPageFinished(v: WebView, url: String?) {
                if (SignInHosts.youtubeOrigin(url?.let(Uri::parse))) v.evaluateJavascript(SCRIPT, null)
            }
        }
        view.webChromeClient = object : WebChromeClient() {
            override fun onReceivedTitle(v: WebView, title: String?) = report(title)
        }
        // Attached but invisible and untouchable: a detached view is throttled, and nothing here is for the user to see.
        view.alpha = 0f
        view.setOnTouchListener { _, _ -> true }
        activity.addContentView(view, FrameLayout.LayoutParams(1, 1))
        web = view
        val timeout = Runnable {
            if (loading) destroy()
        }
        deadline = timeout
        main.postDelayed(timeout, LOAD_TIMEOUT_MS)
        view.loadUrl("https://www.youtube.com/watch?v=$videoId")
    }

    /** `LUNORX_ST|<video id>|<position ms>|<duration ms>|<state>` from the page title. */
    private fun report(title: String?) {
        if (title == null || !title.startsWith(PREFIX)) return
        val parts = title.removePrefix(PREFIX).split("|")
        if (parts.size != 4 || parts[0] != expected) return
        val position = parts[1].toLongOrNull() ?: return
        val duration = parts[2].toLongOrNull() ?: return
        if (parts[3] !in STATES) return
        state = parts[3]
        if (!wanted && state == "playing") call("pause()")
        positionMs = position
        durationMs = duration
        if (state == "ended" || state == "error") holdAwake(false)
        if (shown) refresh()
        if (loading) {
            // Loaded: hold it paused, as core expects, then let reports through.
            loading = false
            deadline?.let(main::removeCallbacks)
            shown = true
            Audio.showPage(mirror)
            call("pause()")
            pending?.resolve()
            pending = null
        }
    }

    private fun call(js: String) {
        web?.evaluateJavascript("window.__lunorx&&window.__lunorx.$js", null)
    }

    fun play() {
        wanted = true
        holdAwake(true)
        call("play()")
        refresh()
    }

    fun pause() {
        wanted = false
        holdAwake(false)
        call("pause()")
        refresh()
    }

    fun seek(ms: Long) = call("seek($ms)")

    fun volume(percent: Int) = call("volume($percent)")

    fun poll(): JSObject {
        val status = when {
            loading -> "loading"
            state == "ended" -> "ended"
            state == "error" -> "failed"
            // Loaded and not yet asked to play: core expects the track paused, whatever the page is doing.
            !wanted -> "paused"
            state == "playing" -> "playing"
            state == "waiting" -> "buffering"
            else -> "paused"
        }
        return JSObject().put("status", status).put("positionMs", positionMs.toDouble()).put("durationMs", durationMs.toDouble())
    }

    fun destroy() {
        holdAwake(false)
        deadline?.let(main::removeCallbacks)
        deadline = null
        pending?.reject("stopped")
        pending = null
        loading = false
        web?.let {
            call("pause()")
            it.stopLoading()
            (it.parent as? ViewGroup)?.removeView(it)
            it.destroy()
        }
        web = null
        if (shown) {
            shown = false
            Audio.showPage(null)
        }
    }

    private companion object {
        const val PREFIX = "LUNORX_ST|"
        const val LOAD_TIMEOUT_MS = 25_000L
        const val WAKE_LIMIT_MS = 6 * 60 * 60 * 1000L
        val STATES = setOf("playing", "paused", "waiting", "ended", "error")

        // Runs inside the page. Reports what the page's own player does and carries out play, pause, seek and volume.
        // It makes no request of its own. If YouTube moves on by itself (its own "up next"), the track that was asked
        // for is over: stop the page and report that, because the app chooses what plays next.
        val SCRIPT = """
            (() => {
              if (window.__lunorx) return;
              const P = "LUNORX_ST|";
              const id = () => new URLSearchParams(location.search).get("v") || "";
              const media = () => document.querySelector("video");
              const state = (m) => (m.ended ? "ended" : m.error ? "error" : m.paused ? "paused" : m.readyState < 3 ? "waiting" : "playing");
              const first = id();
              let last = "";
              // YouTube pauses a page it believes is hidden, which is what a page in the background looks like. This
              // page only ever plays audio, so it says it is visible.
              try {
                Object.defineProperty(document, "hidden", { get: () => false, configurable: true });
                Object.defineProperty(document, "visibilityState", { get: () => "visible", configurable: true });
              } catch (e) { /* the page keeps its own answer */ }
              // The page's ads (user decision 2026-10-31, see AGENTS.md): muted, sped up, skipped where the page lets
              // them be, and never reported as the track. `lastAd` keeps the end of an ad from reading as the end of
              // the episode.
              // Why the page's player stopped, for the device log (console messages reach logcat).
              let watched = null;
              const watch = (m) => {
                if (watched === m) return;
                watched = m;
                for (const type of ["pause", "ended", "error"]) {
                  m.addEventListener(type, () => console.log("LUNORX video " + type + " visibility=" + document.visibilityState + " want=" + want + " ad=" + adShowing() + " t=" + Math.round(m.currentTime)));
                }
              };
              let lastAd = 0;
              let lastPos = 0;
              let lastDur = 0;
              let fast = false;
              const adShowing = () => {
                const player = document.getElementById("movie_player");
                return !!(player && player.classList.contains("ad-showing"));
              };
              const skipAd = (m) => {
                m.muted = true;
                if (m.playbackRate !== 16) { m.playbackRate = 16; fast = true; }
                const skip = document.querySelector(".ytp-skip-ad-button, .ytp-ad-skip-button, .ytp-ad-skip-button-modern, .ytp-skip-ad-button__text");
                if (skip) (skip.closest("button") || skip).click();
                if (Number.isFinite(m.duration) && m.duration > 0 && m.currentTime < m.duration - 0.25) m.currentTime = m.duration;
              };
              // Without a tap the page starts its player muted and says so with an "unmute" button. Core owns the
              // volume, so once it has asked for sound the page is held at the level it chose.
              let want = false;
              let vol = 1;
              const sound = (m) => {
                const player = document.getElementById("movie_player");
                if (vol > 0) {
                  if (player && player.unMute && player.isMuted && player.isMuted()) player.unMute();
                  if (m.muted) m.muted = false;
                } else {
                  m.muted = true;
                }
                if (Math.abs(m.volume - vol) > 0.001) m.volume = vol;
              };
              setInterval(() => {
                const m = media();
                if (!m || !id()) return;
                watch(m);
                if (adShowing()) {
                  lastAd = Date.now();
                  skipAd(m);
                  const line = P + id() + "|" + lastPos + "|" + lastDur + "|waiting";
                  if (line !== last) { last = line; document.title = line; }
                  return;
                }
                if (fast) { m.playbackRate = 1; fast = false; }
                if (want) sound(m);
                if (first && id() !== first) {
                  m.pause();
                  m.muted = true;
                  const line = P + first + "|" + Math.round(m.currentTime * 1000) + "|0|ended";
                  if (line !== last) { last = line; document.title = line; }
                  return;
                }
                const dur = Number.isFinite(m.duration) ? m.duration : 0;
                lastPos = Math.round(m.currentTime * 1000);
                lastDur = Math.round(dur * 1000);
                const shown = Date.now() - lastAd < 2000 ? "waiting" : state(m);
                const line = P + id() + "|" + lastPos + "|" + lastDur + "|" + shown;
                if (line !== last) { last = line; document.title = line; }
              }, 500);
              window.__lunorx = {
                play: () => { want = true; const m = media(); if (m) { sound(m); m.play().catch(() => {}); } },
                pause: () => { want = false; const m = media(); if (m) m.pause(); },
                seek: (ms) => { const m = media(); if (m) m.currentTime = ms / 1000; },
                volume: (n) => { vol = Math.max(0, Math.min(1, n / 100)); const m = media(); if (m && want) sound(m); },
              };
            })();
        """.trimIndent()
    }
}
