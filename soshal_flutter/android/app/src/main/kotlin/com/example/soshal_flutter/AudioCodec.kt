package com.example.soshal_flutter

import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioRecord
import android.media.AudioTrack
import android.media.MediaCodec
import android.media.MediaCodecInfo
import android.media.MediaFormat
import android.media.MediaRecorder
import android.os.Handler
import android.os.HandlerThread
import android.os.Process
import io.flutter.plugin.common.MethodChannel
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.concurrent.ConcurrentLinkedQueue

/**
 * Live audio: AAC-LC encode (mic via AudioRecord + MediaCodec) and decode
 * (MediaCodec + AudioTrack), exposed over the `com.soshal/audio` method
 * channel.
 *
 * Encode: a dedicated handler thread reads 48 kHz mono PCM from the mic,
 * feeds the AAC encoder, and pushes every drained output buffer into a
 * queue as `[tag, ...aac]` — tag `2` = codec config (must be delivered
 * before any frame), tag `1` = audio frame. Dart drains the queue with
 * `drainAudio` and publishes each blob as one MoQ `AudioDatagram` object.
 *
 * Decode: `feedAac` queues a blob into the AAC decoder; drained PCM goes
 * straight to a streaming AudioTrack (started on first frame).
 */
class AudioCodec : MethodChannel.MethodCallHandler {

    private var record: AudioRecord? = null
    private var encoder: MediaCodec? = null
    private var decoder: MediaCodec? = null
    private var track: AudioTrack? = null
    private var audioThread: HandlerThread? = null
    private val outQueue = ConcurrentLinkedQueue<ByteArray>()
    private val sampleRate = 48000
    @Volatile private var encoding = false

    override fun onMethodCall(call: MethodCall, result: MethodChannel.Result) {
        when (call.method) {
            "isSupported" -> result.success(true)
            "initEncode" -> result.success(initEncode())
            "drainAudio" -> result.success(drainAudio())
            "setMicEnable" -> {
                setMicEnable(call.arguments as? Boolean ?: false)
                result.success(true)
            }
            "initDecode" -> result.success(initDecode())
            "feedAac" -> {
                feedAac(call.arguments as? ByteArray)
                result.success(true)
            }
            "release" -> {
                release()
                result.success(true)
            }
            else -> result.notImplemented()
        }
    }

    private fun initEncode(): Boolean {
        try {
            release()
            val minBuf = AudioRecord.getMinBufferSize(
                sampleRate,
                AudioFormat.CHANNEL_IN_MONO,
                AudioFormat.ENCODING_PCM_16BIT,
            )
            if (minBuf <= 0) return false
            record = AudioRecord(
                MediaRecorder.AudioSource.MIC,
                sampleRate,
                AudioFormat.CHANNEL_IN_MONO,
                AudioFormat.ENCODING_PCM_16BIT,
                minBuf * 2,
            )
            if (record?.state != AudioRecord.STATE_INITIALIZED) return false
            val format = MediaFormat.createAudioFormat("audio/mp4a-latm", sampleRate, 1).apply {
                setInteger(
                    MediaFormat.KEY_AAC_PROFILE,
                    MediaCodecInfo.CodecProfileLevel.AACObjectLC,
                )
                setInteger(MediaFormat.KEY_BIT_RATE, 64000)
            }
            encoder = MediaCodec.createEncoderByType("audio/mp4a-latm").also {
                it.configure(format, null, null, MediaCodec.CONFIGURE_FLAG_ENCODE)
                it.start()
            }
            audioThread = HandlerThread(
                "soshal-audio",
                Process.THREAD_PRIORITY_URGENT_AUDIO,
            ).also { it.start() }
            Handler(audioThread!!.looper).post { encodeLoop() }
            return true
        } catch (e: Exception) {
            release()
            return false
        }
    }

    private fun encodeLoop() {
        val codec = encoder ?: return
        val rec = record ?: return
        if (rec.state != AudioRecord.STATE_INITIALIZED) return
        rec.startRecording()
        val chunk = ShortArray(3840) // 40 ms at 48 kHz mono
        val bytes = ByteBuffer.allocate(chunk.size * 2).order(ByteOrder.LITTLE_ENDIAN)
        while (encoding && encoder === codec && record === rec) {
            val n = rec.read(chunk, 0, chunk.size)
            if (n <= 0) continue
            bytes.clear()
            bytes.asShortBuffer().put(chunk, 0, n)
            val payload = bytes.array()
            val inIndex = codec.dequeueInputBuffer(1000)
            if (inIndex >= 0) {
                val buf = codec.getInputBuffer(inIndex) ?: continue
                buf.clear()
                buf.put(payload, 0, n * 2)
                codec.queueInputBuffer(inIndex, 0, n * 2, 0, 0)
            }
            drainEncoder(codec)
        }
        try {
            rec.stop()
        } catch (e: Exception) {
            // already stopped
        }
    }

    private fun drainEncoder(codec: MediaCodec) {
        while (true) {
            val info = MediaCodec.BufferInfo()
            val outIndex = codec.dequeueOutputBuffer(info, 0)
            if (outIndex == MediaCodec.INFO_TRY_AGAIN_LATER) break
            if (outIndex == MediaCodec.INFO_OUTPUT_FORMAT_CHANGED) continue
            try {
                if (info.size > 0) {
                    val buf = codec.getOutputBuffer(outIndex) ?: continue
                    val aac = ByteArray(info.size)
                    buf.position(info.offset)
                    buf.get(aac)
                    val isConfig = info.flags and MediaCodec.BUFFER_FLAG_CODEC_CONFIG != 0
                    LiveRecorder.writeAudio(aac, isConfig)
                    val tag = if (isConfig) 2 else 1
                    val tagged = ByteArray(aac.size + 1)
                    tagged[0] = tag.toByte()
                    System.arraycopy(aac, 0, tagged, 1, aac.size)
                    outQueue.add(tagged)
                }
            } finally {
                codec.releaseOutputBuffer(outIndex, false)
            }
        }
    }

    /** Start/stop the mic. Keeps the codec warm; queue is cleared on start. */
    private fun setMicEnable(on: Boolean) {
        encoding = on
        val rec = record ?: return
        try {
            if (on && rec.state == AudioRecord.STATE_INITIALIZED &&
                rec.recordingState != AudioRecord.RECORDSTATE_RECORDING
            ) {
                outQueue.clear()
                rec.startRecording()
            } else if (!on && rec.recordingState == AudioRecord.RECORDSTATE_RECORDING) {
                rec.stop()
            }
        } catch (e: Exception) {
            // mic state race — ignore
        }
    }

    private fun drainAudio(): List<ByteArray> {
        val out = ArrayList<ByteArray>()
        while (true) {
            val blob = outQueue.poll() ?: break
            out.add(blob)
            if (out.size >= 64) break
        }
        return out
    }

    private fun initDecode(): Boolean {
        try {
            releaseDecoderOnly()
            val minBuf = AudioTrack.getMinBufferSize(
                sampleRate,
                AudioFormat.CHANNEL_OUT_MONO,
                AudioFormat.ENCODING_PCM_16BIT,
            )
            if (minBuf <= 0) return false
            val attributes = AudioAttributes.Builder()
                .setUsage(AudioAttributes.USAGE_MEDIA)
                .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
                .build()
            val format = AudioFormat.Builder()
                .setSampleRate(sampleRate)
                .setEncoding(AudioFormat.ENCODING_PCM_16BIT)
                .setChannelMask(AudioFormat.CHANNEL_OUT_MONO)
                .build()
            track = AudioTrack.Builder()
                .setAudioAttributes(attributes)
                .setAudioFormat(format)
                .setBufferSizeInBytes(minBuf * 2)
                .setTransferMode(AudioTrack.MODE_STREAM)
                .build()
            decoder = MediaCodec.createDecoderByType("audio/mp4a-latm").also {
                it.configure(
                    MediaFormat.createAudioFormat("audio/mp4a-latm", sampleRate, 1),
                    null,
                    null,
                    0,
                )
                it.start()
            }
            return true
        } catch (e: Exception) {
            releaseDecoderOnly()
            return false
        }
    }

    private fun feedAac(blob: ByteArray?) {
        if (blob == null || blob.isEmpty()) return
        val codec = decoder ?: return
        val aTrack = track ?: return
        try {
            val inIndex = codec.dequeueInputBuffer(1000)
            if (inIndex >= 0) {
                val buf = codec.getInputBuffer(inIndex) ?: return
                buf.clear()
                buf.put(blob)
                codec.queueInputBuffer(inIndex, 0, blob.size, 0, 0)
            }
            while (true) {
                val info = MediaCodec.BufferInfo()
                val outIndex = codec.dequeueOutputBuffer(info, 0)
                if (outIndex == MediaCodec.INFO_TRY_AGAIN_LATER) break
                if (outIndex == MediaCodec.INFO_OUTPUT_FORMAT_CHANGED) continue
                try {
                    val buf = codec.getOutputBuffer(outIndex) ?: continue
                    if (info.size > 0) {
                        if (aTrack.playState != AudioTrack.PLAYSTATE_PLAYING) aTrack.play()
                        val pcm = ByteArray(info.size)
                        buf.position(info.offset)
                        buf.get(pcm)
                        aTrack.write(pcm, 0, pcm.size)
                    }
                } finally {
                    codec.releaseOutputBuffer(outIndex, false)
                }
            }
        } catch (e: Exception) {
            // dropped chunk
        }
    }

    private fun releaseDecoderOnly() {
        try {
            decoder?.stop()
            decoder?.release()
        } catch (e: Exception) {
            // already released
        }
        try {
            track?.stop()
            track?.release()
        } catch (e: Exception) {
            // already released
        }
        decoder = null
        track = null
    }

    private fun release() {
        encoding = false
        val rec = record
        try {
            rec?.stop()
            rec?.release()
        } catch (e: Exception) {
            // already released
        }
        record = null
        try {
            encoder?.stop()
            encoder?.release()
        } catch (e: Exception) {
            // already released
        }
        encoder = null
        audioThread?.quitSafely()
        audioThread = null
        outQueue.clear()
        releaseDecoderOnly()
    }
}