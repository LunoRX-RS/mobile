package io.lunorx.nativeplugin

import android.content.Intent
import androidx.media3.session.MediaSession
import androidx.media3.session.MediaSessionService

/**
 * Keeps audio alive in the background and shows the system media notification / lock-screen controls. The player is
 * owned by [Audio]; this service only publishes it. Notification buttons reach core through [Audio]'s forwarding
 * player, never the ExoPlayer directly.
 */
class PlaybackService : MediaSessionService() {
    private var session: MediaSession? = null

    override fun onCreate() {
        super.onCreate()
        // The service must be exported for the system media UI, so any app could otherwise bind and press play, seek or
        // skip. Only trusted controllers connect: this app, system apps (notification, lock screen, Bluetooth) and the
        // media notification itself.
        session = MediaSession.Builder(this, Audio.controls(this))
            .setCallback(object : MediaSession.Callback {
                override fun onConnect(session: MediaSession, controller: MediaSession.ControllerInfo): MediaSession.ConnectionResult =
                    if (controller.packageName == packageName || controller.isTrusted || session.isMediaNotificationController(controller)) {
                        MediaSession.ConnectionResult.AcceptedResultBuilder(session).build()
                    } else {
                        MediaSession.ConnectionResult.reject()
                    }
            })
            .build()
    }

    override fun onGetSession(controllerInfo: MediaSession.ControllerInfo): MediaSession? = session

    override fun onTaskRemoved(rootIntent: Intent?) {
        Audio.stopForTaskRemoval()
        stopSelf()
    }

    override fun onDestroy() {
        session?.release()
        session = null
        super.onDestroy()
    }
}
