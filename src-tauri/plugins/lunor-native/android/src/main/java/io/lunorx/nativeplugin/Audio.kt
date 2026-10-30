package io.lunorx.nativeplugin

import android.content.Context
import androidx.media3.common.AudioAttributes
import androidx.media3.common.C
import androidx.media3.common.ForwardingPlayer
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.Player
import androidx.media3.datasource.DefaultDataSource
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.source.DefaultMediaSourceFactory
import androidx.media3.session.MediaSession

/** One ExoPlayer for the process. Core drives it through [LunorNativePlugin]; the system UI only queues [remote] events. */
object Audio {
    private var exo: ExoPlayer? = null
    private var forwarding: Player? = null
    private var appContext: Context? = null

    /** The service's media session, so the page player can put its own state in front of ExoPlayer's. */
    @Volatile var session: MediaSession? = null

    /** Set while a track plays through the page player; the session then shows this instead of ExoPlayer. */
    private var page: Player? = null
    private val events = ArrayDeque<Pair<String, Double>>()

    var ended = false
    var failed = false
    var loaded = false

    fun player(context: Context): ExoPlayer =
        exo ?: ExoPlayer.Builder(context.applicationContext)
            // Files are read from disk; everything else comes from the loopback proxy (see [LoopbackDataSource]).
            .setMediaSourceFactory(
                DefaultMediaSourceFactory(
                    DefaultDataSource.Factory(context.applicationContext, LoopbackDataSource.Factory()),
                ),
            )
            .setAudioAttributes(
                AudioAttributes.Builder()
                    .setUsage(C.USAGE_MEDIA)
                    .setContentType(C.AUDIO_CONTENT_TYPE_MUSIC)
                    .build(),
                true,
            )
            .setHandleAudioBecomingNoisy(true)
            .setWakeMode(C.WAKE_MODE_LOCAL)
            .build()
            .also { player ->
                player.addListener(object : Player.Listener {
                    override fun onPlaybackStateChanged(state: Int) {
                        if (state == Player.STATE_ENDED) ended = true
                    }

                    override fun onPlayerError(error: androidx.media3.common.PlaybackException) {
                        failed = true
                    }

                    override fun onPlayWhenReadyChanged(playWhenReady: Boolean, reason: Int) {
                        // Audio focus loss / noisy route pauses ExoPlayer itself; tell core so state stays truthful.
                        if (!playWhenReady && reason != Player.PLAY_WHEN_READY_CHANGE_REASON_USER_REQUEST) {
                            push("pause")
                        }
                    }
                })
                exo = player
            }

    /** What the MediaSession exposes. Transport actions become events for core instead of acting on ExoPlayer. */
    fun controls(context: Context): Player {
        appContext = context.applicationContext
        return page ?: exoControls(context)
    }

    /** Main thread. Shows `mirror` in the media session, or ExoPlayer again for `null`. */
    fun showPage(mirror: Player?) {
        page = mirror
        val context = appContext ?: return
        session?.player = mirror ?: exoControls(context)
    }

    private fun exoControls(context: Context): Player =
        forwarding ?: object : ForwardingPlayer(player(context)) {
            override fun play() = push("play")
            override fun pause() = push("pause")
            override fun seekToNext() = push("next")
            override fun seekToPrevious() = push("previous")
            override fun seekToNextMediaItem() = push("next")
            override fun seekToPreviousMediaItem() = push("previous")
            override fun seekTo(positionMs: Long) = push("seek", positionMs.toDouble())
            override fun seekTo(mediaItemIndex: Int, positionMs: Long) = push("seek", positionMs.toDouble())
            override fun getAvailableCommands(): Player.Commands =
                super.getAvailableCommands().buildUpon()
                    .add(Player.COMMAND_SEEK_TO_NEXT)
                    .add(Player.COMMAND_SEEK_TO_PREVIOUS)
                    .add(Player.COMMAND_SEEK_IN_CURRENT_MEDIA_ITEM)
                    .build()
            override fun isCommandAvailable(command: Int): Boolean =
                command == Player.COMMAND_SEEK_TO_NEXT ||
                    command == Player.COMMAND_SEEK_TO_PREVIOUS ||
                    super.isCommandAvailable(command)
        }.also { forwarding = it }

    fun push(kind: String, positionMs: Double = 0.0) {
        synchronized(events) {
            if (events.size < 32) events.addLast(kind to positionMs)
        }
    }

    fun drain(): List<Pair<String, Double>> = synchronized(events) {
        val out = events.toList()
        events.clear()
        out
    }

    fun mediaItem(url: String, title: String? = null, artist: String? = null): MediaItem =
        MediaItem.Builder()
            .setUri(url)
            .setMediaMetadata(
                MediaMetadata.Builder()
                    .setTitle(title)
                    .setArtist(artist)
                    .setMediaType(MediaMetadata.MEDIA_TYPE_MUSIC)
                    .setIsPlayable(true)
                    .build(),
            )
            .build()

    fun stopForTaskRemoval() {
        exo?.pause()
        push("pause")
    }
}
