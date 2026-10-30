package io.lunorx.nativeplugin

import android.app.Activity
import android.app.Dialog
import android.graphics.Color
import android.view.ViewGroup
import android.webkit.CookieManager
import android.webkit.WebResourceRequest
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.Button
import android.widget.LinearLayout
import android.content.ComponentName
import android.net.Uri
import android.os.Handler
import android.os.Looper
import androidx.media3.session.MediaController
import androidx.media3.session.SessionToken
import com.google.common.util.concurrent.ListenableFuture
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import androidx.media3.common.MediaMetadata
import androidx.media3.common.Player
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin

@InvokeArg
class LoadArgs {
    lateinit var url: String
}

@InvokeArg
class PageLoadArgs {
    lateinit var videoId: String
}

@InvokeArg
class ImportArgs {
    var state: String = "running"
}

@InvokeArg
class SeekArgs {
    var positionMs: Double = 0.0
}

@InvokeArg
class VolumeArgs {
    var volume: Double = 100.0
}

@InvokeArg
class NowPlayingArgs {
    var title: String = ""
    var artist: String = ""
    var artworkUrl: String? = null
    var durationMs: Double = 0.0
    var positionMs: Double = 0.0
    var playing: Boolean = false
}

@InvokeArg
class SecretArgs {
    lateinit var service: String
    lateinit var account: String
    var value: String? = null
}

/**
 * Same JSON contract as the iOS plugin. ExoPlayer calls run on the main thread. Core owns playback state; this class
 * reports what the player does and queues system-UI presses as `remote` events.
 */
@TauriPlugin
class LunorNativePlugin(private val activity: Activity) : Plugin(activity) {
    private val main = Handler(Looper.getMainLooper())
    private val secrets by lazy { SecretStore(activity) }
    private val page by lazy { PagePlayer(activity, main) }
    private var pendingLoad: Invoke? = null
    private var pendingPlay: Invoke? = null

    private val listener = object : Player.Listener {
        override fun onPlaybackStateChanged(state: Int) {
            if (state == Player.STATE_READY) {
                pendingLoad?.resolve()
                pendingLoad = null
            }
        }

        override fun onIsPlayingChanged(isPlaying: Boolean) {
            if (isPlaying) {
                pendingPlay?.resolve()
                pendingPlay = null
            }
        }

        override fun onPlayerError(error: androidx.media3.common.PlaybackException) {
            pendingLoad?.reject("load failed")
            pendingLoad = null
            pendingPlay?.reject("play failed")
            pendingPlay = null
        }
    }

    override fun load(webView: android.webkit.WebView) {
        super.load(webView)
        main.post {
            Audio.player(activity).addListener(listener)
            bindPlaybackService()
            requestNotificationPermission()
        }
        publishInsets(webView)
    }

    /**
     * Android WebView reports `env(safe-area-inset-*)` as 0 when the app draws edge to edge, so the tab bar would sit under
     * the gesture bar. The system-bar and cutout insets are written to `--sai-*` instead; the stylesheet takes the larger
     * of the two. The page loads after this hook runs, so the values are written again a little later.
     */
    private fun publishInsets(webView: android.webkit.WebView) {
        var script = ""
        fun push() = webView.evaluateJavascript(script, null)
        ViewCompat.setOnApplyWindowInsetsListener(webView) { _, insets ->
            val bars = insets.getInsets(WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout())
            val density = webView.resources.displayMetrics.density
            fun css(px: Int) = "${px / density}px"
            script = "(function(){var s=document.documentElement.style;" +
                "s.setProperty('--sai-top','${css(bars.top)}');s.setProperty('--sai-bottom','${css(bars.bottom)}');" +
                "s.setProperty('--sai-left','${css(bars.left)}');s.setProperty('--sai-right','${css(bars.right)}');})()"
            push()
            // A cold start can take longer than a few seconds to load the page, and a navigation drops the values.
            for (i in 1..60) main.postDelayed({ push() }, i * 500L)
            insets
        }
        ViewCompat.requestApplyInsets(webView)
    }

    private fun onMain(invoke: Invoke, block: () -> Unit) {
        main.post {
            try {
                block()
            } catch (e: Exception) {
                invoke.reject("native error")
            }
        }
    }

    private fun cancelPending(reason: String) {
        pendingLoad?.reject(reason)
        pendingLoad = null
        pendingPlay?.reject(reason)
        pendingPlay = null
    }

    // Binding a MediaController is what brings PlaybackService up. Media3 then moves it to the foreground itself while
    // audio plays. Calling startForegroundService here instead would kill the app after about ten seconds whenever the
    // player is not playing yet ("did not call startForeground in time").
    private var controller: ListenableFuture<MediaController>? = null

    /**
     * Android 13+ shows the media notification, and with it the lock-screen and Live Alert media controls, only after the
     * user allows notifications. Asked once at launch; a refusal just leaves the controls out.
     */
    private fun requestNotificationPermission() {
        if (android.os.Build.VERSION.SDK_INT < 33) return
        val permission = android.Manifest.permission.POST_NOTIFICATIONS
        if (activity.checkSelfPermission(permission) != android.content.pm.PackageManager.PERMISSION_GRANTED) {
            activity.requestPermissions(arrayOf(permission), 4101)
        }
    }

    private fun bindPlaybackService() {
        if (controller != null) return
        val token = SessionToken(activity, ComponentName(activity, PlaybackService::class.java))
        controller = MediaController.Builder(activity, token).buildAsync()
    }

    @Command
    fun load(invoke: Invoke) {
        val args = invoke.parseArgs(LoadArgs::class.java)
        val uri = Uri.parse(args.url)
        if (!(uri.scheme == "file" || (uri.scheme == "http" && uri.host == "127.0.0.1") || (uri.scheme == "https" && uri.host?.endsWith(".googlevideo.com") == true))) {
            invoke.reject("url not allowed")
            return
        }
        onMain(invoke) {
            page.destroy()
            cancelPending("superseded")
            Audio.ended = false
            Audio.failed = false
            Audio.loaded = true
            val player = Audio.player(activity)
            pendingLoad = invoke
            player.playWhenReady = false
            player.setMediaItem(Audio.mediaItem(args.url))
            player.prepare()
        }
    }

    /**
     * Plays a YouTube video through its own page when the direct stream was refused. Core checks the id; it is checked
     * again here, and the address is built here, so no caller supplies a URL. While the page holds the track, play,
     * pause, seek, setVolume, stop and poll act on it.
     */
    @Command
    fun pageLoad(invoke: Invoke) {
        val args = invoke.parseArgs(PageLoadArgs::class.java)
        if (!Regex("[A-Za-z0-9_-]{11}").matches(args.videoId)) {
            invoke.reject("id not allowed")
            return
        }
        onMain(invoke) {
            cancelPending("superseded")
            val player = Audio.player(activity)
            player.stop()
            player.clearMediaItems()
            Audio.ended = false
            Audio.failed = false
            Audio.loaded = false
            page.load(args.videoId, invoke)
        }
    }

    @Command
    fun play(invoke: Invoke) {
        onMain(invoke) {
            if (page.active) {
                page.play()
                invoke.resolve()
                return@onMain
            }
            val player = Audio.player(activity)
            if (!Audio.loaded || Audio.failed) {
                invoke.reject("nothing loaded")
            } else if (player.isPlaying) {
                invoke.resolve()
            } else {
                pendingPlay?.reject("superseded")
                pendingPlay = invoke
                player.play()
            }
        }
    }

    @Command
    fun pause(invoke: Invoke) {
        onMain(invoke) {
            if (page.active) {
                page.pause()
                invoke.resolve()
                return@onMain
            }
            pendingPlay?.reject("superseded")
            pendingPlay = null
            Audio.player(activity).pause()
            invoke.resolve()
        }
    }

    @Command
    fun stop(invoke: Invoke) {
        onMain(invoke) {
            page.destroy()
            cancelPending("stopped")
            val player = Audio.player(activity)
            player.stop()
            player.clearMediaItems()
            Audio.ended = false
            Audio.failed = false
            Audio.loaded = false
            invoke.resolve()
        }
    }

    @Command
    fun seek(invoke: Invoke) {
        val args = invoke.parseArgs(SeekArgs::class.java)
        onMain(invoke) {
            if (page.active) {
                page.seek(args.positionMs.toLong().coerceAtLeast(0))
                invoke.resolve()
            } else if (!Audio.loaded) {
                invoke.reject("nothing loaded")
            } else {
                // ExoPlayer.seekTo is applied synchronously from the caller's view; position reads back at once.
                Audio.player(activity).seekTo(args.positionMs.toLong().coerceAtLeast(0))
                invoke.resolve()
            }
        }
    }

    @Command
    fun setVolume(invoke: Invoke) {
        val args = invoke.parseArgs(VolumeArgs::class.java)
        onMain(invoke) {
            val percent = args.volume.coerceIn(0.0, 100.0)
            if (page.active) page.volume(percent.toInt())
            Audio.player(activity).volume = (percent / 100.0).toFloat()
            invoke.resolve()
        }
    }

    @Command
    fun importProgress(invoke: Invoke) {
        val args = invoke.parseArgs(ImportArgs::class.java)
        ImportNotice.show(activity, args.state)
        invoke.resolve()
    }

    @Command
    fun poll(invoke: Invoke) {
        onMain(invoke) {
            if (page.active) {
                val reply = page.poll()
                val events = JSArray()
                for ((kind, positionMs) in Audio.drain()) {
                    events.put(JSObject().put("kind", kind).put("positionMs", positionMs))
                }
                invoke.resolve(reply.put("remote", events))
                return@onMain
            }
            val player = Audio.player(activity)
            val status = when {
                !Audio.loaded -> "idle"
                Audio.failed -> "failed"
                Audio.ended -> "ended"
                player.playbackState == Player.STATE_IDLE -> "loading"
                player.playbackState == Player.STATE_BUFFERING -> if (player.playWhenReady) "buffering" else "loading"
                player.isPlaying -> "playing"
                else -> "paused"
            }
            val events = JSArray()
            for ((kind, positionMs) in Audio.drain()) {
                events.put(JSObject().put("kind", kind).put("positionMs", positionMs))
            }
            val reply = JSObject()
            reply.put("status", status)
            reply.put("positionMs", player.currentPosition.coerceAtLeast(0).toDouble())
            reply.put("durationMs", player.duration.let { if (it < 0) 0.0 else it.toDouble() })
            reply.put("remote", events)
            invoke.resolve(reply)
        }
    }

    @Command
    fun nowPlaying(invoke: Invoke) {
        val args = invoke.parseArgs(NowPlayingArgs::class.java)
        onMain(invoke) {
            val player = Audio.player(activity)
            val current = player.currentMediaItem
            if (current != null) {
                val meta = MediaMetadata.Builder()
                    .setTitle(args.title)
                    .setArtist(args.artist)
                    .setMediaType(MediaMetadata.MEDIA_TYPE_MUSIC)
                    .setIsPlayable(true)
                    .setArtworkUri(args.artworkUrl?.takeIf { it.startsWith("https://") }?.let(Uri::parse))
                    .build()
                player.replaceMediaItem(0, current.buildUpon().setMediaMetadata(meta).build())
            }
            invoke.resolve()
        }
    }

    /**
     * Full-screen Google sign-in. Resolves with the YouTube / Google cookies once the user is signed in, or an empty list if the user cancels.
     * Core validates and stores them; nothing is kept here. The host allowlist matches the desktop window
     * (`login_navigation` in src/commands/youtube.rs): keep them in step. The WebView cookie jar is process-wide, so it is
     * emptied before and after.
     */
    @Command
    fun signIn(invoke: Invoke) {
        onMain(invoke) {
            val cookies = CookieManager.getInstance()
            cookies.removeAllCookies(null)
            val web = WebView(activity)
            with(web.settings) {
                javaScriptEnabled = true
                domStorageEnabled = true
                allowFileAccess = false
                allowContentAccess = false
                setSupportMultipleWindows(false)
                javaScriptCanOpenWindowsAutomatically = false
                // The default WebView agent carries "; wv", which Google refuses for sign-in.
                userAgentString = userAgentString.replace("; wv", "")
            }
            cookies.setAcceptThirdPartyCookies(web, false)
            val dialog = Dialog(activity, android.R.style.Theme_Black_NoTitleBar_Fullscreen)
            var done = false
            var polling = false
            fun finish(result: JSArray?) {
                if (done) return
                done = true
                web.stopLoading()
                cookies.removeAllCookies(null)
                cookies.flush()
                web.clearCache(true)
                android.webkit.WebStorage.getInstance().deleteAllData()
                dialog.dismiss()
                web.destroy()
                // Cancelling is a normal outcome, reported as an empty list; reject is for real failures.
                invoke.resolve(JSObject().put("cookies", result ?: JSArray()))
            }
            fun jar(): JSArray {
                val out = JSArray()
                for ((url, domain) in listOf(
                    "https://www.youtube.com" to "youtube.com",
                    "https://m.youtube.com" to "youtube.com",
                    "https://music.youtube.com" to "youtube.com",
                    "https://www.google.com" to "google.com",
                )) {
                    cookies.getCookie(url)?.split(";")?.forEach { pair ->
                        val i = pair.indexOf('=')
                        if (i > 0) out.put(JSObject().put("name", pair.substring(0, i).trim()).put("value", pair.substring(i + 1).trim()).put("domain", domain))
                    }
                }
                return out
            }
            fun poll(attempt: Int) {
                if (done) return
                val found = jar()
                val names = (0 until found.length()).map { found.getJSONObject(it).getString("name") }
                if (names.any { it == "SAPISID" || it == "__Secure-1PAPISID" || it == "__Secure-3PAPISID" }) finish(found)
                else if (attempt < 9) main.postDelayed({ poll(attempt + 1) }, 1500)
                else polling = false
            }
            web.webViewClient = object : WebViewClient() {
                override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean =
                    !SignInHosts.allowed(request.url)

                override fun onPageFinished(view: WebView, url: String?) {
                    if (!polling && SignInHosts.youtubeOrigin(url?.let(Uri::parse))) {
                        polling = true
                        poll(0)
                    }
                }
            }
            val cancel = Button(activity).apply { text = "Cancel"; setOnClickListener { finish(null) } }
            val column = LinearLayout(activity).apply {
                orientation = LinearLayout.VERTICAL
                setBackgroundColor(Color.BLACK)
                fitsSystemWindows = true
                addView(cancel, LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT))
                addView(web, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
            }
            dialog.setContentView(column)
            dialog.setOnCancelListener { finish(null) }
            dialog.show()
            web.loadUrl("https://accounts.google.com/ServiceLogin?service=youtube&continue=https%3A%2F%2Fmusic.youtube.com%2F")
        }
    }

    @Command
    fun secretGet(invoke: Invoke) {
        val args = invoke.parseArgs(SecretArgs::class.java)
        try {
            val reply = JSObject()
            secrets.get(args.service, args.account)?.let { reply.put("value", it) }
            invoke.resolve(reply)
        } catch (e: Exception) {
            invoke.reject("secret store")
        }
    }

    @Command
    fun secretSet(invoke: Invoke) {
        val args = invoke.parseArgs(SecretArgs::class.java)
        try {
            secrets.set(args.service, args.account, args.value ?: throw IllegalArgumentException())
            invoke.resolve()
        } catch (e: Exception) {
            invoke.reject("secret store")
        }
    }

    @Command
    fun secretDelete(invoke: Invoke) {
        val args = invoke.parseArgs(SecretArgs::class.java)
        try {
            secrets.delete(args.service, args.account)
            invoke.resolve()
        } catch (e: Exception) {
            invoke.reject("secret store")
        }
    }
}

/** Same rules as `login_navigation` / `youtube_origin` in src/commands/youtube.rs. */
object SignInHosts {
    private fun plain(uri: Uri) = uri.scheme == "https" && uri.port == -1 && uri.userInfo == null

    /** YouTube answers a phone user agent with `m.youtube.com`; every other `*.youtube.com` host is the same service. */
    private fun youtubeHost(host: String) = host == "youtube.com" || host.endsWith(".youtube.com")

    /**
     * `google.com` and its subdomains (accounts, consent, the One Google Bar), plus the account's country domain in
     * the forms `google.<cc>`, `google.co.<cc>` and `google.com.<cc>` with a two-letter code. Lookalikes such as
     * `notgoogle.com` or `google.co.th.evil.test` do not pass.
     */
    private fun googleHost(host: String): Boolean {
        val labels = host.split(".")
        val index = labels.indexOf("google")
        if (index < 0) return false
        val rest = labels.subList(index + 1, labels.size)
        val code = { c: String -> c.length == 2 && c.all { it in 'a'..'z' } }
        return when (rest.size) {
            1 -> rest[0] == "com" || code(rest[0])
            2 -> (rest[0] == "co" || rest[0] == "com") && code(rest[1])
            else -> false
        }
    }

    fun youtubeOrigin(uri: Uri?): Boolean =
        uri != null && plain(uri) && uri.host?.lowercase()?.let(::youtubeHost) == true

    fun allowed(uri: Uri): Boolean {
        if (uri.toString() == "about:blank") return true
        if (!plain(uri)) return false
        val host = uri.host?.lowercase() ?: return false
        return youtubeHost(host) || googleHost(host)
    }
}
