package io.lunorx.nativeplugin

import android.Manifest
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build

/**
 * Progress of the YouTube library import. On Android 16 it is requested as a Live Update (status-bar chip and lock
 * screen); older versions show an ordinary progress notification. The import has no percentage, so the bar is indeterminate.
 */
object ImportNotice {
    private const val CHANNEL = "lunor_import"
    private const val ID = 4102

    fun show(context: Context, state: String) {
        if (Build.VERSION.SDK_INT >= 33 &&
            context.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) return
        val manager = context.getSystemService(NotificationManager::class.java) ?: return
        manager.createNotificationChannel(NotificationChannel(CHANNEL, "YouTube import", NotificationManager.IMPORTANCE_LOW))
        val running = state == "running"
        val text = when (state) {
            "running" -> "Importing your YouTube Music library"
            "done" -> "YouTube Music library imported"
            else -> "YouTube Music import failed"
        }
        val open = context.packageManager.getLaunchIntentForPackage(context.packageName)?.let {
            PendingIntent.getActivity(context, 0, it, PendingIntent.FLAG_IMMUTABLE)
        }
        val builder = Notification.Builder(context, CHANNEL)
            .setSmallIcon(context.applicationInfo.icon)
            .setContentTitle("LUNOR-X")
            .setContentText(text)
            .setCategory(Notification.CATEGORY_PROGRESS)
            .setOngoing(running)
            .setOnlyAlertOnce(true)
            .setAutoCancel(!running)
            .setContentIntent(open)
        if (running) {
            if (Build.VERSION.SDK_INT >= 36) {
                builder.setStyle(Notification.ProgressStyle().setProgressIndeterminate(true))
                    .setShortCriticalText("Import")
                // The documented opt-in to a Live Update; the extra is what Notification.Builder sets for it.
                builder.extras.putBoolean("android.requestPromotedOngoing", true)
            } else {
                builder.setProgress(0, 0, true)
            }
        } else {
            builder.setTimeoutAfter(6000)
        }
        manager.notify(ID, builder.build())
    }
}
