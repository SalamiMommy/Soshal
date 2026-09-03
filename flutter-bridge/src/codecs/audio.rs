//! Live audio: AAudio mic capture → AAC-LC encoder, AAC decoder → AAudio
//! playback, via NDK (Android only). Encode runs on a dedicated Rust thread;
//! drained blobs are queued as `[tag, ...aac]` (tag 2 = codec config, tag 1
//! = frame) and mirrored to the Kotlin LiveRecorder DVR via JNI.

#![allow(unsafe_code)]

use std::sync::atomic::{AtomicBool, Ordering};

use super::audio_state;
#[cfg(target_os = "android")]
use super::ndk::*;

#[allow(dead_code)] // android-only cfg callers
const SAMPLE_RATE: i32 = 48_000;
#[allow(dead_code)]
const CHUNK_FRAMES: i32 = 3840; // 40 ms at 48 kHz mono

/// Doubles as mic enable: capture thread idles when false, records when true.
static MIC_ENABLED: AtomicBool = AtomicBool::new(false);
static CAPTURE_STOP: AtomicBool = AtomicBool::new(false);

/// True only on Android ≥ 26 (AAudio + AImage gate).
pub fn is_supported() -> bool {
    #[cfg(target_os = "android")]
    {
        super::sdk_gate()
    }
    #[cfg(not(target_os = "android"))]
    {
        false
    }
}

/// Release all audio resources and join capture thread safely without deadlocking.
pub fn release_audio_all() {
    CAPTURE_STOP.store(true, Ordering::SeqCst);
    MIC_ENABLED.store(false, Ordering::SeqCst);
    let join_thread = {
        let mut s = audio_state();
        s.capture_thread.take()
    };
    if let Some(t) = join_thread {
        let _ = t.join();
    }
    let mut s = audio_state();
    s.release_audio();
}

/// Start mic + AAC-LC encoder + capture thread. Caller must
/// `set_mic_enable(true)` before audio flows.
pub fn init_encode() -> bool {
    #[cfg(target_os = "android")]
    {
        if !super::sdk_gate() {
            return false;
        }
        release_audio_all();
        CAPTURE_STOP.store(false, Ordering::SeqCst);
        let mut s = audio_state();

        unsafe {
            // AAC-LC encoder (config blob is drained first by MediaCodec).
            let codec = AMediaCodec_createEncoderByType(c"audio/mp4a-latm".as_ptr());
            if codec.is_null() {
                return false;
            }
            let fmt = AMediaFormat_new();
            AMediaFormat_setString(fmt, c"mime".as_ptr(), c"audio/mp4a-latm".as_ptr());
            AMediaFormat_setInt32(fmt, c"sample-rate".as_ptr(), SAMPLE_RATE);
            AMediaFormat_setInt32(fmt, c"channel-count".as_ptr(), 1);
            AMediaFormat_setInt32(fmt, c"aac-profile".as_ptr(), 2); // AACObjectLC
            AMediaFormat_setInt32(fmt, c"bitrate".as_ptr(), 64_000);
            let ok = AMediaCodec_configure(
                codec,
                fmt,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                AMEDIACODEC_CONFIGURE_FLAG_ENCODE,
            ) == 0
                && AMediaCodec_start(codec) == 0;
            AMediaFormat_delete(fmt);
            if !ok {
                AMediaCodec_delete(codec);
                return false;
            }

            // AAudio input stream.
            let mut builder = std::ptr::null_mut();
            if AAudioStreamBuilder_create(&mut builder) != AAUDIO_OK {
                AMediaCodec_delete(codec);
                return false;
            }
            AAudioStreamBuilder_setDirection(builder, AAUDIO_DIRECTION_INPUT);
            AAudioStreamBuilder_setFormat(builder, AAUDIO_FORMAT_PCM_I16);
            AAudioStreamBuilder_setSampleRate(builder, SAMPLE_RATE);
            AAudioStreamBuilder_setChannelCount(builder, 1);
            AAudioStreamBuilder_setInputPreset(builder, AAUDIO_INPUT_PRESET_VOICE_RECOGNITION);
            AAudioStreamBuilder_setPerformanceMode(builder, AAUDIO_PERFORMANCE_MODE_LOW_LATENCY);
            let mut mic = std::ptr::null_mut();
            let rc = AAudioStreamBuilder_openStream(builder, &mut mic);
            AAudioStreamBuilder_delete(builder);
            if rc != AAUDIO_OK || mic.is_null() {
                AMediaCodec_delete(codec);
                return false;
            }
            s.audio_encoder = Some(super::NativeCodec(codec));
            s.mic = Some(super::NativeStream(mic));
        }

        MIC_ENABLED.store(false, Ordering::Relaxed);
        let join = std::thread::Builder::new()
            .name("soshal-audio-capture".to_string())
            .spawn(capture_loop)
            .ok();
        s.capture_thread = join;
        s.audio_queue.clear();
        true
    }
    #[cfg(not(target_os = "android"))]
    {
        false
    }
}

/// Toggle the mic (keeps the codec warm). Queue is cleared on start.
pub fn set_mic_enable(on: bool) -> bool {
    let prev = MIC_ENABLED.swap(on, Ordering::Relaxed);
    if on && !prev {
        let mut s = audio_state();
        s.audio_queue.clear();
    }
    true
}

/// Drain queued AAC blobs: `[2, ...config]` or `[1, ...frame]`.
pub fn drain() -> Vec<Vec<u8>> {
    let mut s = audio_state();
    let mut out = Vec::new();
    while out.len() < 64 {
        match s.audio_queue.pop_front() {
            Some(blob) => out.push(blob),
            None => break,
        }
    }
    out
}

/// Set up AAC decoder + AAudio output. Safe to call once per viewer session.
pub fn init_decode() -> bool {
    #[cfg(target_os = "android")]
    {
        if !super::sdk_gate() {
            return false;
        }
        let mut s = audio_state();
        s.release_audio_decoder_only();
        unsafe {
            let codec = AMediaCodec_createDecoderByType(c"audio/mp4a-latm".as_ptr());
            if codec.is_null() {
                return false;
            }
            let fmt = AMediaFormat_new();
            AMediaFormat_setString(fmt, c"mime".as_ptr(), c"audio/mp4a-latm".as_ptr());
            AMediaFormat_setInt32(fmt, c"sample-rate".as_ptr(), SAMPLE_RATE);
            AMediaFormat_setInt32(fmt, c"channel-count".as_ptr(), 1);
            let ok =
                AMediaCodec_configure(codec, fmt, std::ptr::null_mut(), std::ptr::null_mut(), 0)
                    == 0
                    && AMediaCodec_start(codec) == 0;
            AMediaFormat_delete(fmt);
            if !ok {
                AMediaCodec_delete(codec);
                return false;
            }

            let mut builder = std::ptr::null_mut();
            if AAudioStreamBuilder_create(&mut builder) != AAUDIO_OK {
                AMediaCodec_delete(codec);
                return false;
            }
            AAudioStreamBuilder_setDirection(builder, AAUDIO_DIRECTION_OUTPUT);
            AAudioStreamBuilder_setFormat(builder, AAUDIO_FORMAT_PCM_I16);
            AAudioStreamBuilder_setSampleRate(builder, SAMPLE_RATE);
            AAudioStreamBuilder_setChannelCount(builder, 1);
            AAudioStreamBuilder_setPerformanceMode(builder, AAUDIO_PERFORMANCE_MODE_LOW_LATENCY);
            let mut speaker = std::ptr::null_mut();
            let rc = AAudioStreamBuilder_openStream(builder, &mut speaker);
            AAudioStreamBuilder_delete(builder);
            if rc != AAUDIO_OK || speaker.is_null() {
                AMediaCodec_delete(codec);
                return false;
            }
            s.audio_decoder = Some(super::NativeCodec(codec));
            s.speaker = Some(super::NativeStream(speaker));
        }
        true
    }
    #[cfg(not(target_os = "android"))]
    {
        false
    }
}

/// Feed one AAC blob (config or frame); decoded PCM plays immediately.
pub fn feed_aac(blob: &[u8]) -> bool {
    #[cfg(target_os = "android")]
    {
        if blob.is_empty() {
            return false;
        }
        let s = audio_state();
        let (Some(codec), Some(speaker)) = (s.audio_decoder.as_ref(), s.speaker.as_ref()) else {
            return false;
        };
        let codec = codec.0;
        let speaker = speaker.0;
        unsafe {
            let idx = AMediaCodec_dequeueInputBuffer(codec, 1000);
            if idx < 0 {
                return false;
            }
            let mut size = 0usize;
            let buf = AMediaCodec_getInputBuffer(codec, idx as usize, &mut size);
            if buf.is_null() || size < blob.len() {
                AMediaCodec_queueInputBuffer(codec, idx as usize, 0, 0, 0, 0);
                return false;
            }
            std::ptr::copy_nonoverlapping(blob.as_ptr(), buf, blob.len());
            AMediaCodec_queueInputBuffer(codec, idx as usize, 0, blob.len(), 0, 0);

            let mut info = AMediaCodecBufferInfo {
                offset: 0,
                size: 0,
                presentation_time_us: 0,
                flags: 0,
            };
            loop {
                let out_idx = AMediaCodec_dequeueOutputBuffer(codec, &mut info, 0);
                if out_idx == AMEDIACODEC_INFO_TRY_AGAIN_LATER {
                    break;
                }
                if out_idx == AMEDIACODEC_INFO_OUTPUT_FORMAT_CHANGED
                    || out_idx == AMEDIACODEC_INFO_OUTPUT_BUFFERS_CHANGED
                {
                    continue;
                }
                if out_idx < 0 {
                    break;
                }
                let mut out_size = 0usize;
                let out_buf = AMediaCodec_getOutputBuffer(codec, out_idx as usize, &mut out_size);
                if !out_buf.is_null()
                    && info.size > 0
                    && info.offset >= 0
                    && (info.offset as usize).saturating_add(info.size as usize) <= out_size
                {
                    let pcm = std::slice::from_raw_parts(
                        out_buf.offset(info.offset as isize) as *const i16,
                        (info.size as usize) / 2,
                    );
                    AAudioStream_requestStart(speaker);
                    AAudioStream_write(
                        speaker,
                        pcm.as_ptr() as *const std::ffi::c_void,
                        pcm.len() as i32,
                        10_000_000,
                    );
                }
                AMediaCodec_releaseOutputBuffer(codec, out_idx as usize, 0);
            }
        }
        true
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = blob;
        false
    }
}

/// Stop mic, codecs, playback; release everything.
pub fn release() -> bool {
    release_audio_all();
    true
}

/// Capture + encode loop, runs on `soshal-audio-capture` thread. Pushes
/// tagged blobs straight into the state queue (drain() pops them).
#[cfg(target_os = "android")]
fn capture_loop() {
    let mut chunk = vec![0i16; CHUNK_FRAMES as usize];
    loop {
        if CAPTURE_STOP.load(Ordering::SeqCst) {
            break;
        }
        let (codec, mic) = {
            let s = audio_state();
            (
                s.audio_encoder.as_ref().map(|c| c.0),
                s.mic.as_ref().map(|m| m.0),
            )
        };
        let (Some(codec), Some(mic)) = (codec, mic) else {
            break;
        };
        if !MIC_ENABLED.load(Ordering::Relaxed) {
            unsafe {
                AAudioStream_requestStop(mic);
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
            continue;
        }
        unsafe {
            AAudioStream_requestStart(mic);
            let read = AAudioStream_read(
                mic,
                chunk.as_mut_ptr() as *mut std::ffi::c_void,
                CHUNK_FRAMES,
                50_000_000, // 50 ms
            );
            if read <= 0 {
                continue;
            }
            let read = read.min(CHUNK_FRAMES);
            let bytes: &[u8] =
                std::slice::from_raw_parts(chunk.as_ptr() as *const u8, (read * 2) as usize);
            let idx = AMediaCodec_dequeueInputBuffer(codec, 1000);
            if idx >= 0 {
                let mut size = 0usize;
                let buf = AMediaCodec_getInputBuffer(codec, idx as usize, &mut size);
                if !buf.is_null() && size >= bytes.len() {
                    std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, bytes.len());
                    AMediaCodec_queueInputBuffer(codec, idx as usize, 0, bytes.len(), 0, 0);
                } else {
                    AMediaCodec_queueInputBuffer(codec, idx as usize, 0, 0, 0, 0);
                }
            }
            drain_audio_encoder(codec);
        }
    }
    unsafe {
        let s = audio_state();
        if let Some(mic) = s.mic.as_ref() {
            AAudioStream_requestStop(mic.0);
        }
    }
}

/// Drain the AAC encoder into the state queue (and the DVR muxer).
#[cfg(target_os = "android")]
unsafe fn drain_audio_encoder(codec: *mut AMediaCodec) {
    let mut info = AMediaCodecBufferInfo {
        offset: 0,
        size: 0,
        presentation_time_us: 0,
        flags: 0,
    };
    for _ in 0..8 {
        let idx = AMediaCodec_dequeueOutputBuffer(codec, &mut info, 0);
        if idx == AMEDIACODEC_INFO_TRY_AGAIN_LATER {
            break;
        }
        if idx == AMEDIACODEC_INFO_OUTPUT_FORMAT_CHANGED
            || idx == AMEDIACODEC_INFO_OUTPUT_BUFFERS_CHANGED
        {
            continue;
        }
        if idx < 0 {
            break;
        }
        let mut size = 0usize;
        let buf = AMediaCodec_getOutputBuffer(codec, idx as usize, &mut size);
        if !buf.is_null()
            && info.size > 0
            && info.offset >= 0
            && (info.offset as usize).saturating_add(info.size as usize) <= size
        {
            let aac =
                std::slice::from_raw_parts(buf.offset(info.offset as isize), info.size as usize);
            let is_config = info.flags & AMEDIACODEC_BUFFER_FLAG_CODEC_CONFIG != 0;
            let _ = super::dvr_write_audio(aac, is_config); // DVR mirror; ignore failure
            let tag: u8 = if is_config { 2 } else { 1 };
            let mut tagged = Vec::with_capacity(aac.len() + 1);
            tagged.push(tag);
            tagged.extend_from_slice(aac);
            let mut s = audio_state();
            // Bound the queue to prevent unbounded memory growth under backpressure.
            const MAX_AUDIO_QUEUE: usize = 128;
            if s.audio_queue.len() >= MAX_AUDIO_QUEUE {
                s.audio_queue.pop_front(); // drop oldest frame
            }
            s.audio_queue.push_back(tagged);
        }
        AMediaCodec_releaseOutputBuffer(codec, idx as usize, 0);
    }
}
