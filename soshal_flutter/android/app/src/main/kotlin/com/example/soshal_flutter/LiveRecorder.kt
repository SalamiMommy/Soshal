package com.example.soshal_flutter

import android.content.Context
import android.media.MediaCodec
import android.media.MediaFormat
import android.media.MediaMuxer
import java.io.File
import java.nio.ByteBuffer

/**
 * Local DVR for live broadcasts: multiplexes the in-flight H.264 + AAC
 * bleed (from `H264Codec` / `AudioCodec` drains) into an MP4 file under
 * `<filesDir>/recordings/`.
 *
 * Tracks are added lazily — video waits for the first key-frame buffer and
 * its SPS/PPS (`csd-0`, captured from the encoder's config output or its
 * output MediaFormat), audio waits for the codec-config blob (also written
 * as `csd-0`). All writes go through one lock: the encoder drains can run
 * on the main thread (video) and the audio thread simultaneously.
 *
 * Wire protocol over the codec channels: `initRecord()` -> absolute output
 * path (null = failure), `stopRecord()` -> recorded file path.
 */
object LiveRecorder {

    private var context: Context? = null
    private var muxer: MediaMuxer? = null
    private var outPath: String? = null
    private var videoTrack = -1
    private var audioTrack = -1
    private var videoCsd: ByteArray? = null
    private var audioCsd: ByteArray? = null
    private var started = false
    private var baseUs = 0L
    private val lock = Any()

    @Volatile
    var recording: Boolean = false
        private set

    fun init(applicationContext: Context) {
        context = applicationContext
    }

    fun start(): String? = synchronized(lock) {
        if (recording) return@synchronized outPath
        val ctx = context ?: return@synchronized null
        try {
            val dir = File(ctx.filesDir, "recordings").apply { mkdirs() }
            if (!dir.isDirectory) return@synchronized null
            val path = File(dir, "soshal_${System.currentTimeMillis()}.mp4")
                .absolutePath
            muxer = MediaMuxer(path, MediaMuxer.OutputFormat.MUXER_OUTPUT_MPEG_4)
            outPath = path
            videoTrack = -1
            audioTrack = -1
            videoCsd = null
            audioCsd = null
            started = false
            baseUs = System.nanoTime() / 1000
            recording = true
            path
        } catch (e: Exception) {
            recording = false
            null
        }
    }

    fun stop(): String? = synchronized(lock) {
        if (!recording) return@synchronized outPath
        recording = false
        try {
            muxer?.stop()
        } catch (e: Exception) {
            // few samples only — keep the partial file anyway
        }
        try {
            muxer?.release()
        } catch (e: Exception) {
            // already released
        }
        muxer = null
        val path = outPath
        if (path != null && File(path).length() <= 0) {
            File(path).delete()
            outPath = null
        }
        path
    }

    /** Video sample from the H.264 encoder drain (Rust codecs → JNI). */
    fun writeVideo(nal: ByteArray, isKey: Boolean, isConfig: Boolean, width: Int, height: Int) {
        synchronized(lock) {
            if (!recording) return
            if (isConfig) {
                videoCsd = nal
                return
            }
            val mx = muxer ?: return
            if (videoTrack < 0) {
                val csd = videoCsd ?: return // SPS/PPS not seen yet — drop
                val format = MediaFormat.createVideoFormat("video/avc", width, height).apply {
                    setByteBuffer("csd-0", ByteBuffer.wrap(csd))
                }
                try {
                    videoTrack = mx.addTrack(format)
                } catch (e: Exception) {
                    // muxer already started (audio came first) — drop video
                    return
                }
                maybeStart(mx)
            }
            writeSample(mx, videoTrack, nal, isKey)
        }
    }

    /** Audio sample (config blob or AAC frame) from the AAC encoder drain. */
    fun writeAudio(blob: ByteArray, isConfig: Boolean) {
        synchronized(lock) {
            if (!recording) return
            if (isConfig) {
                audioCsd = blob
                val mx = muxer ?: return
                if (audioTrack < 0) {
                    val format = MediaFormat.createAudioFormat(
                        "audio/mp4a-latm",
                        AUDIO_SAMPLE_RATE,
                        AUDIO_CHANNELS,
                    ).apply {
                        setByteBuffer("csd-0", ByteBuffer.wrap(blob))
                        setInteger(MediaFormat.KEY_BIT_RATE, 64000)
                    }
                    try {
                        audioTrack = mx.addTrack(format)
                    } catch (e: Exception) {
                        // muxer already started (video came first) — drop audio
                        return
                    }
                    maybeStart(mx)
                }
                return
            }
            val mx = muxer ?: return
            if (audioTrack < 0) return // config not seen yet — drop
            writeSample(mx, audioTrack, blob, false)
        }
    }

    private fun maybeStart(mx: MediaMuxer) {
        if (started) return
        if (videoTrack < 0 && audioTrack < 0) return
        try {
            mx.start()
            started = true
        } catch (e: Exception) {
            // start() on a stopped/dry muxer — leave started false
        }
    }

    private fun writeSample(
        mx: MediaMuxer,
        track: Int,
        data: ByteArray,
        isKey: Boolean,
    ) {
        val nowUs = System.nanoTime() / 1000 - baseUs
        if (nowUs < 0) return
        val info = MediaCodec.BufferInfo().apply {
            offset = 0
            size = data.size
            presentationTimeUs = nowUs
            flags = if (isKey) MediaCodec.BUFFER_FLAG_KEY_FRAME else 0
        }
        try {
            mx.writeSampleData(track, ByteBuffer.wrap(data), info)
        } catch (e: Exception) {
            // muxer stopped mid-write — drop rest
        }
    }

    const val AUDIO_SAMPLE_RATE = 48000
    const val AUDIO_CHANNELS = 1
}