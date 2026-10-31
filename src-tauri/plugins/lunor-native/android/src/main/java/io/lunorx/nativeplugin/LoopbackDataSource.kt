package io.lunorx.nativeplugin

import android.net.Uri
import androidx.media3.common.C
import androidx.media3.common.PlaybackException
import androidx.media3.common.util.UnstableApi
import androidx.media3.datasource.BaseDataSource
import androidx.media3.datasource.DataSource
import androidx.media3.datasource.DataSpec
import androidx.media3.datasource.HttpDataSource
import java.io.BufferedInputStream
import java.io.IOException
import java.io.InputStream
import java.net.InetSocketAddress
import java.net.Socket

/**
 * Reads audio from the app's own loopback proxy (`http://127.0.0.1:<port>/<path>`) over a plain socket.
 *
 * Android's network security policy blocks cleartext http even to the loopback address, and the app cannot relax it:
 * the TLS verifier library already supplies the app's one policy file. A raw socket is not covered by that policy, and
 * nothing leaves the device here, so the proxy is read directly. Only the loopback address is accepted; anything else
 * is refused, which also means no other http(s) address can be played through the native player.
 */
@UnstableApi
class LoopbackDataSource : BaseDataSource(/* isNetwork = */ false) {
    private var socket: Socket? = null
    private var input: InputStream? = null
    private var opened: Uri? = null
    private var remaining = C.LENGTH_UNSET.toLong()

    override fun open(dataSpec: DataSpec): Long {
        val uri = dataSpec.uri
        if (uri.scheme != "http" || uri.host != "127.0.0.1" || uri.port !in 1..65535) {
            throw IOException("only the loopback proxy can be read")
        }
        opened = uri
        transferInitializing(dataSpec)
        val connection = Socket()
        try {
            connection.connect(InetSocketAddress("127.0.0.1", uri.port), CONNECT_MS)
            connection.soTimeout = READ_MS
            val range = if (dataSpec.length != C.LENGTH_UNSET.toLong()) {
                "bytes=${dataSpec.position}-${dataSpec.position + dataSpec.length - 1}"
            } else {
                "bytes=${dataSpec.position}-"
            }
            val path = (uri.encodedPath ?: "/") + (uri.encodedQuery?.let { "?$it" } ?: "")
            connection.getOutputStream().apply {
                write("GET $path HTTP/1.1\r\nHost: 127.0.0.1:${uri.port}\r\nRange: $range\r\nConnection: close\r\n\r\n".toByteArray(Charsets.US_ASCII))
                flush()
            }
            val stream = BufferedInputStream(connection.getInputStream(), BUFFER)
            val status = readLine(stream).split(' ').getOrNull(1)?.toIntOrNull() ?: 0
            var length = C.LENGTH_UNSET.toLong()
            while (true) {
                val line = readLine(stream)
                if (line.isEmpty()) break
                val colon = line.indexOf(':')
                if (colon > 0 && line.substring(0, colon).trim().equals("content-length", ignoreCase = true)) {
                    length = line.substring(colon + 1).trim().toLongOrNull() ?: length
                }
            }
            if (status != 200 && status != 206) {
                throw HttpDataSource.InvalidResponseCodeException(
                    status, null, null, emptyMap(), dataSpec, ByteArray(0),
                )
            }
            if (status == 200 && dataSpec.position > 0) throw IOException("the proxy ignored the range")
            socket = connection
            input = stream
            remaining = length
        } catch (e: IOException) {
            runCatching { connection.close() }
            throw e
        }
        transferStarted(dataSpec)
        return remaining
    }

    override fun read(buffer: ByteArray, offset: Int, length: Int): Int {
        if (length == 0) return 0
        if (remaining == 0L) return C.RESULT_END_OF_INPUT
        val wanted = if (remaining == C.LENGTH_UNSET.toLong()) length else minOf(length.toLong(), remaining).toInt()
        val read = input?.read(buffer, offset, wanted) ?: return C.RESULT_END_OF_INPUT
        if (read < 0) {
            // The proxy closed early: ExoPlayer treats that as a failed read, not as the end of the file.
            if (remaining != C.LENGTH_UNSET.toLong()) throw IOException("the proxy closed with $remaining bytes left")
            return C.RESULT_END_OF_INPUT
        }
        if (remaining != C.LENGTH_UNSET.toLong()) remaining -= read
        bytesTransferred(read)
        return read
    }

    override fun getUri(): Uri? = opened

    override fun close() {
        val wasOpen = socket != null
        runCatching { socket?.close() }
        socket = null
        input = null
        opened = null
        remaining = C.LENGTH_UNSET.toLong()
        if (wasOpen) transferEnded()
    }

    private fun readLine(stream: InputStream): String {
        val line = StringBuilder()
        while (line.length < MAX_LINE) {
            val b = stream.read()
            if (b < 0) throw IOException("the proxy closed before answering")
            if (b == '\n'.code) break
            if (b != '\r'.code) line.append(b.toChar())
        }
        return line.toString()
    }

    class Factory : DataSource.Factory {
        override fun createDataSource(): DataSource = LoopbackDataSource()
    }

    private companion object {
        const val CONNECT_MS = 5_000
        const val READ_MS = 30_000
        const val BUFFER = 64 * 1024
        const val MAX_LINE = 8192
    }
}
