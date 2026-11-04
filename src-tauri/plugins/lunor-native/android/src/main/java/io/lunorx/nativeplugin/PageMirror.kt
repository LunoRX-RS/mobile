package io.lunorx.nativeplugin

import android.net.Uri
import android.os.Looper
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.Player
import androidx.media3.common.SimpleBasePlayer
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture

/**
 * What the system media UI shows while a track plays through the page player. The page makes the sound; this only
 * mirrors its state into the media session, so the notification, the lock screen and the foreground service behave as
 * they do for ExoPlayer, and Android sees the app's audio as media playback. Buttons never act here: like the
 * ExoPlayer wrapper in [Audio], they become events for core.
 */
class PageMirror : SimpleBasePlayer(Looper.getMainLooper()) {
    private var playing = false
    private var buffering = false
    private var ended = false
    private var positionMs = 0L
    private var durationMs = 0L
    private var title: String? = null
    private var artist: String? = null
    private var artwork: Uri? = null

    fun update(playing: Boolean, buffering: Boolean, ended: Boolean, positionMs: Long, durationMs: Long) {
        this.playing = playing
        this.buffering = buffering
        this.ended = ended
        this.positionMs = positionMs
        this.durationMs = durationMs
        invalidateState()
    }

    fun describe(title: String?, artist: String?, artworkUrl: String?) {
        this.title = title
        this.artist = artist
        this.artwork = artworkUrl?.takeIf { it.startsWith("https://") }?.let(Uri::parse)
        invalidateState()
    }

    override fun getState(): State {
        val metadata = MediaMetadata.Builder()
            .setTitle(title)
            .setArtist(artist)
            .setArtworkUri(artwork)
            .setMediaType(MediaMetadata.MEDIA_TYPE_MUSIC)
            .setIsPlayable(true)
            .build()
        val item = MediaItem.Builder().setMediaId(ID).setMediaMetadata(metadata).build()
        val data = MediaItemData.Builder(ID)
            .setMediaItem(item)
            .setMediaMetadata(metadata)
            .setDurationUs(if (durationMs > 0) durationMs * 1000 else C.TIME_UNSET)
            .setIsSeekable(durationMs > 0)
            .build()
        val commands = Player.Commands.Builder()
            .addAll(
                Player.COMMAND_PLAY_PAUSE,
                Player.COMMAND_PREPARE,
                Player.COMMAND_STOP,
                Player.COMMAND_SEEK_IN_CURRENT_MEDIA_ITEM,
                Player.COMMAND_SEEK_TO_NEXT,
                Player.COMMAND_SEEK_TO_PREVIOUS,
                Player.COMMAND_GET_CURRENT_MEDIA_ITEM,
                Player.COMMAND_GET_TIMELINE,
                Player.COMMAND_GET_METADATA,
            )
            .build()
        return State.Builder()
            .setAvailableCommands(commands)
            .setPlayWhenReady(playing, Player.PLAY_WHEN_READY_CHANGE_REASON_USER_REQUEST)
            .setPlaybackState(
                when {
                    ended -> Player.STATE_ENDED
                    buffering -> Player.STATE_BUFFERING
                    else -> Player.STATE_READY
                },
            )
            .setPlaylist(listOf(data))
            .setCurrentMediaItemIndex(0)
            .setContentPositionMs(positionMs)
            .build()
    }

    override fun handleSetPlayWhenReady(playWhenReady: Boolean): ListenableFuture<*> {
        Audio.push(if (playWhenReady) "play" else "pause")
        return Futures.immediateVoidFuture()
    }

    override fun handleSeek(mediaItemIndex: Int, positionMs: Long, seekCommand: Int): ListenableFuture<*> {
        when (seekCommand) {
            Player.COMMAND_SEEK_TO_NEXT, Player.COMMAND_SEEK_TO_NEXT_MEDIA_ITEM -> Audio.push("next")
            Player.COMMAND_SEEK_TO_PREVIOUS, Player.COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM -> Audio.push("previous")
            else -> Audio.push("seek", positionMs.toDouble())
        }
        return Futures.immediateVoidFuture()
    }

    override fun handlePrepare(): ListenableFuture<*> = Futures.immediateVoidFuture()

    override fun handleStop(): ListenableFuture<*> = Futures.immediateVoidFuture()

    private companion object {
        const val ID = "page"
    }
}
