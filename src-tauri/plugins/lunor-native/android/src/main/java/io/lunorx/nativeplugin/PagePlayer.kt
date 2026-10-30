package io.lunorx.nativeplugin

import android.app.Activity
import android.net.Uri
import android.os.Handler
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

    val active: Boolean get() = web != null

    fun load(videoId: String, invoke: Invoke) {
        destroy()
        expected = videoId
        loading = true
        state = "waiting"
        wanted = false
        positionMs = 0
        durationMs = 0
        pending = invoke
        val view = WebView(activity)
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
        if (loading) {
            // Loaded: hold it paused, as core expects, then let reports through.
            loading = false
            deadline?.let(main::removeCallbacks)
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
        call("play()")
    }

    fun pause() {
        wanted = false
        call("pause()")
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
    }

    private companion object {
        const val PREFIX = "LUNORX_ST|"
        const val LOAD_TIMEOUT_MS = 25_000L
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
                if (want) sound(m);
                if (first && id() !== first) {
                  m.pause();
                  m.muted = true;
                  const line = P + first + "|" + Math.round(m.currentTime * 1000) + "|0|ended";
                  if (line !== last) { last = line; document.title = line; }
                  return;
                }
                const dur = Number.isFinite(m.duration) ? m.duration : 0;
                const line = P + id() + "|" + Math.round(m.currentTime * 1000) + "|" + Math.round(dur * 1000) + "|" + state(m);
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
