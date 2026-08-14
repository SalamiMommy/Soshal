package com.example.soshal_flutter

import android.graphics.ImageFormat
import android.graphics.Rect
import android.graphics.YuvImage
import android.media.Image
import android.media.MediaCodec
import android.media.MediaCodecInfo
import android.media.MediaCodecList
import android.media.MediaFormat
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel
import java.io.ByteArrayOutputStream

/**
 * Hardware H.264 (MediaCodec) encode + decode, exposed over the
 * `com.soshal/h264` method channel.
 *
 * Encode: BGRA frames (from the camera plugin's bgra8888 stream) are
 * converted to I420, fed to the hardware AVC encoder, and every drained
 * output buffer is returned as `[flag, ...Annex-B NALs]` (flag 1 = key
 * frame). Decode: Annex-B NAL blobs (one MoQ object per drain) are fed to
 * the software AVC decoder; each drained frame is converted to a JPEG
 * ByteArray. All work happens on the main thread; 640x480@15fps stays well
 * inside the per-frame budget.
 */
class H264Codec : MethodChannel.MethodCallHandler {

    private var encoder: MediaCodec? = null
    private var decoder: MediaCodec? = null

    override fun onMethodCall(call: MethodCall, result: MethodChannel.Result) {
        when (call.method) {
            "isSupported" -> result.success(h264EncoderAvailable())
            "initEncode" -> result.success(initEncode(call))
            "feedEncode" -> result.success(feedEncode(call.arguments as? ByteArray))
            "initDecode" -> result.success(initDecode())
            "feedDecode" -> result.success(feedDecode(call.arguments as? ByteArray))
            "initRecord" -> result.success(LiveRecorder.start())
            "stopRecord" -> result.success(LiveRecorder.stop())
            "release" -> {
                release()
                result.success(true)
            }
            else -> result.notImplemented()
        }
    }

    private fun h264EncoderAvailable(): Boolean {
        val codecInfo = MediaCodecList(MediaCodecList.ALL_CODECS).codecInfos
        for (info in codecInfo) {
            if (!info.isEncoder) continue
            if (info.supportedTypes.any { it.equals("video/avc", true) }) return true
        }
        return false
    }

    private fun initEncode(call: MethodCall): Boolean {
        val width = call.argument<Int>("width") ?: 0
        val height = call.argument<Int>("height") ?: 0
        val bitrate = call.argument<Int>("bitrate") ?: 800_000
        val fps = call.argument<Int>("fps") ?: 15
        if (width <= 0 || height <= 0) return false
        try {
            release()
            val format = MediaFormat.createVideoFormat("video/avc", width, height).apply {
                setInteger(
                    MediaFormat.KEY_COLOR_FORMAT,
                    MediaCodecInfo.CodecCapabilities.COLOR_FormatYUV420Flexible,
                )
                setInteger(MediaFormat.KEY_BIT_RATE, bitrate)
                setInteger(MediaFormat.KEY_FRAME_RATE, fps)
                setInteger(MediaFormat.KEY_I_FRAME_INTERVAL, 1)
            }
            encoder = MediaCodec.createEncoderByType("video/avc").also {
                it.configure(format, null, null, MediaCodec.CONFIGURE_FLAG_ENCODE)
                it.start()
            }
            return true
        } catch (e: Exception) {
            release()
            return false
        }
    }

    private fun feedEncode(bgra: ByteArray?): List<ByteArray> {
        val codec = encoder ?: return emptyList()
        if (bgra == null || bgra.isEmpty()) return emptyList()
        try {
            val width = codec.inputFormat.getInteger(MediaFormat.KEY_WIDTH)
            val height = codec.inputFormat.getInteger(MediaFormat.KEY_HEIGHT)
            if (bgra.size < width * height * 4) return emptyList()
            val i420 = bgraToI420(bgra, width, height)
            var inIndex = codec.dequeueInputBuffer(0)
            if (inIndex >= 0) {
                val buf = codec.getInputBuffer(inIndex) ?: return drainEncode()
                buf.clear()
                buf.put(i420)
                codec.queueInputBuffer(inIndex, 0, i420.size, 0, 0)
            }
            return drainEncode()
        } catch (e: Exception) {
            return emptyList()
        }
    }

    private fun drainEncode(): List<ByteArray> {
        val codec = encoder ?: return emptyList()
        val out = ArrayList<ByteArray>()
        while (true) {
            val info = MediaCodec.BufferInfo()
            val outIndex = codec.dequeueOutputBuffer(info, 0)
            if (outIndex == MediaCodec.INFO_TRY_AGAIN_LATER) break
            if (outIndex == MediaCodec.INFO_OUTPUT_FORMAT_CHANGED) {
                LiveRecorder.setVideoFormat(codec.outputFormat)
                continue
            }
            try {
                if (info.size > 0) {
                    val buf = codec.getOutputBuffer(outIndex) ?: continue
                    val payload = ByteArray(info.size)
                    buf.position(info.offset)
                    buf.get(payload)
                    val isKey = info.flags and MediaCodec.BUFFER_FLAG_KEY_FRAME != 0
                    val isConfig = info.flags and MediaCodec.BUFFER_FLAG_CODEC_CONFIG != 0
                    LiveRecorder.writeVideo(payload, isKey, isConfig)
                    val flag = if (isKey) 1 else 0
                    val tagged = ByteArray(payload.size + 1)
                    tagged[0] = flag.toByte()
                    System.arraycopy(payload, 0, tagged, 1, payload.size)
                    out.add(tagged)
                }
            } finally {
                codec.releaseOutputBuffer(outIndex, false)
            }
        }
        return out
    }

    private fun initDecode(): Boolean {
        try {
            release()
            decoder = MediaCodec.createDecoderByType("video/avc").also {
                it.configure(
                    MediaFormat.createVideoFormat("video/avc", 0, 0),
                    null,
                    null,
                    0,
                )
                it.start()
            }
            return true
        } catch (e: Exception) {
            release()
            return false
        }
    }

    private fun feedDecode(nal: ByteArray?): List<ByteArray> {
        val codec = decoder ?: return emptyList()
        if (nal == null || nal.isEmpty()) return emptyList()
        try {
            var inIndex = codec.dequeueInputBuffer(0)
            if (inIndex >= 0) {
                val buf = codec.getInputBuffer(inIndex) ?: return emptyList()
                buf.clear()
                buf.put(nal)
                codec.queueInputBuffer(inIndex, 0, nal.size, 0, 0)
            }
            val frames = ArrayList<ByteArray>()
            while (true) {
                val info = MediaCodec.BufferInfo()
                val outIndex = codec.dequeueOutputBuffer(info, 0)
                if (outIndex == MediaCodec.INFO_TRY_AGAIN_LATER) break
                if (outIndex == MediaCodec.INFO_OUTPUT_FORMAT_CHANGED) continue
                try {
                    val image = codec.getOutputImage(outIndex)
                    val jpeg = imageToJpeg(image)
                    if (jpeg != null) frames.add(jpeg)
                } finally {
                    codec.releaseOutputBuffer(outIndex, false)
                }
            }
            return frames
        } catch (e: Exception) {
            return emptyList()
        }
    }

    /** YUV_420_888 -> NV21 -> JPEG q60. Returns null when the frame is unusable. */
    private fun imageToJpeg(image: Image?): ByteArray? {
        if (image == null) return null
        try {
            val crop = image.cropRect
            val w = crop.width()
            val h = crop.height()
            if (w % 2 != 0 || h % 2 != 0 || w <= 0 || h <= 0) return null
            val planes = image.planes
            if (planes.size < 3) return null
            val yPlane = planes[0]
            val uPlane = planes[1]
            val vPlane = planes[2]
            val nv21 = ByteArray(w * h + w * h / 2)
            copyPlane(yPlane, nv21, 0, crop, w, h, 1)
            val uBuf = uPlane.buffer
            val vBuf = vPlane.buffer
            val uvRowStride = uPlane.rowStride
            val vRowStride = vPlane.rowStride
            val uvPixelStride = uPlane.pixelStride
            val vPixelStride = vPlane.pixelStride
            val uvW = w / 2
            val uvH = h / 2
            val uvOffset = w * h
            var uPos = 0
            var vPos = 0
            var nvPos = uvOffset
            for (row in 0 until uvH) {
                uPos = uPlane.offsetInBytes + row * uvRowStride
                vPos = vPlane.offsetInBytes + row * vRowStride
                for (col in 0 until uvW) {
                    val u = uBuf.get(uPos).toInt() and 0xFF
                    val v = vBuf.get(vPos).toInt() and 0xFF
                    nv21[nvPos++] = v.toByte()
                    nv21[nvPos++] = u.toByte()
                    uPos += uvPixelStride
                    vPos += vPixelStride
                }
            }
            val yuv = YuvImage(nv21, ImageFormat.NV21, w, h, null)
            val baos = ByteArrayOutputStream()
            yuv.compressToJpeg(Rect(0, 0, w, h), 60, baos)
            return baos.toByteArray()
        } catch (e: Exception) {
            return null
        } finally {
            image.close()
        }
    }

    private fun copyPlane(
        plane: Image.Plane,
        dst: ByteArray,
        dstOffset: Int,
        crop: Rect,
        w: Int,
        h: Int,
        pixelStride: Int,
    ) {
        val src = plane.buffer
        val rowStride = plane.rowStride
        var srcPos = plane.offsetInBytes + crop.top * rowStride + crop.left * pixelStride
        var dstPos = dstOffset
        for (row in 0 until h) {
            var s = srcPos
            for (col in 0 until w) {
                dst[dstPos++] = src.get(s).toByte()
                s += pixelStride
            }
            srcPos += rowStride
        }
    }

    /** BT.601 studio-swing BGRA (from camera bgra8888) -> packed planar I420. */
    private fun bgraToI420(bgra: ByteArray, width: Int, height: Int): ByteArray {
        val w2 = width / 2
        val h2 = height / 2
        val i420 = ByteArray(width * height + w2 * h2 * 2)
        var yPos = 0
        var uPos = width * height
        var vPos = uPos + w2 * h2
        var p = 0
        for (y in 0 until height) {
            val rowEven = y % 2 == 0
            for (x in 0 until width) {
                val b = bgra[p++].toInt() and 0xFF
                val g = bgra[p++].toInt() and 0xFF
                val r = bgra[p++].toInt() and 0xFF
                p++ // alpha
                i420[yPos++] = (((66 * r + 129 * g + 25 * b + 128) shr 8) + 16).toByte()
                if (rowEven && x % 2 == 0) {
                    i420[uPos++] = (((-38 * r - 74 * g + 112 * b + 128) shr 8) + 128).toByte()
                    i420[vPos++] = (((112 * r - 94 * g - 18 * b + 128) shr 8) + 128).toByte()
                }
            }
        }
        return i420
    }

    private fun release() {
        try {
            encoder?.stop()
            encoder?.release()
        } catch (e: Exception) {
            // already released
        }
        try {
            decoder?.stop()
            decoder?.release()
        } catch (e: Exception) {
            // already released
        }
        encoder = null
        decoder = null
    }
}
