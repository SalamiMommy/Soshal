//! Live media codecs: H.264 + AAC via NDK AMediaCodec/AAudio (Android) with
//! pure-Rust JPEG encode. The MP4 DVR muxer (LiveRecorder) stays Kotlin —
//! MediaMuxer has no NDK API — and is driven from here via JNI.

#![allow(unsafe_code)]

use std::collections::VecDeque;
use std::sync::{LazyLock, Mutex};
use std::thread::JoinHandle;

pub mod audio;
pub mod h264;
#[cfg(target_os = "android")]
mod ndk;

/// Raw NDK handles (only valid on Android). Wrapped in Send so the global
/// state mutex compiles on both targets.
#[cfg(target_os = "android")]
pub struct NativeCodec(*mut ndk::AMediaCodec);
#[cfg(target_os = "android")]
pub struct NativeStream(*mut ndk::AAudioStream);
#[cfg(not(target_os = "android"))]
pub type NativeCodec = usize;
#[cfg(not(target_os = "android"))]
pub type NativeStream = usize;

#[cfg(target_os = "android")]
unsafe impl Send for NativeCodec {}
#[cfg(target_os = "android")]
unsafe impl Send for NativeStream {}

#[derive(Default)]
pub(crate) struct VideoCodecState {
    pub h264_encoder: Option<NativeCodec>,
    pub h264_decoder: Option<NativeCodec>,
    pub h264_width: i32,
    pub h264_height: i32,
}

#[derive(Default)]
pub(crate) struct AudioCodecState {
    pub audio_encoder: Option<NativeCodec>,
    pub audio_decoder: Option<NativeCodec>,
    pub mic: Option<NativeStream>,
    pub speaker: Option<NativeStream>,
    pub audio_queue: VecDeque<Vec<u8>>,
    pub capture_thread: Option<JoinHandle<()>>,
}

static VIDEO_STATE: LazyLock<Mutex<VideoCodecState>> =
    LazyLock::new(|| Mutex::new(VideoCodecState::default()));
static AUDIO_STATE: LazyLock<Mutex<AudioCodecState>> =
    LazyLock::new(|| Mutex::new(AudioCodecState::default()));

pub(crate) fn video_state() -> std::sync::MutexGuard<'static, VideoCodecState> {
    VIDEO_STATE.lock().unwrap_or_else(|e| e.into_inner())
}

pub(crate) fn audio_state() -> std::sync::MutexGuard<'static, AudioCodecState> {
    AUDIO_STATE.lock().unwrap_or_else(|e| e.into_inner())
}

impl VideoCodecState {
    pub fn release_h264_encoder_only(&mut self) {
        #[cfg(target_os = "android")]
        unsafe {
            use ndk::*;
            if let Some(c) = self.h264_encoder.take() {
                AMediaCodec_stop(c.0);
                AMediaCodec_delete(c.0);
            }
        }
        #[cfg(not(target_os = "android"))]
        {
            self.h264_encoder = None;
        }
    }

    pub fn release_h264_decoder_only(&mut self) {
        #[cfg(target_os = "android")]
        unsafe {
            use ndk::*;
            if let Some(c) = self.h264_decoder.take() {
                AMediaCodec_stop(c.0);
                AMediaCodec_delete(c.0);
            }
        }
        #[cfg(not(target_os = "android"))]
        {
            self.h264_decoder = None;
        }
    }

    pub fn release_h264(&mut self) {
        self.release_h264_encoder_only();
        self.release_h264_decoder_only();
        self.h264_width = 0;
        self.h264_height = 0;
    }
}

impl AudioCodecState {
    pub fn release_audio_decoder_only(&mut self) {
        #[cfg(target_os = "android")]
        unsafe {
            use ndk::*;
            if let Some(c) = self.audio_decoder.take() {
                AMediaCodec_stop(c.0);
                AMediaCodec_delete(c.0);
            }
            if let Some(s) = self.speaker.take() {
                AAudioStream_close(s.0);
            }
        }
        #[cfg(not(target_os = "android"))]
        {
            self.audio_decoder = None;
            self.speaker = None;
        }
    }

    pub fn release_audio(&mut self) {
        #[cfg(target_os = "android")]
        {
            if let Some(t) = self.capture_thread.take() {
                let _ = t.join();
            }
        }
        #[cfg(target_os = "android")]
        unsafe {
            use ndk::*;
            if let Some(m) = self.mic.take() {
                AAudioStream_requestStop(m.0);
                AAudioStream_close(m.0);
            }
            if let Some(c) = self.audio_encoder.take() {
                AMediaCodec_stop(c.0);
                AMediaCodec_delete(c.0);
            }
        }
        #[cfg(not(target_os = "android"))]
        {
            self.mic = None;
            self.audio_encoder = None;
            self.capture_thread = None;
        }
        self.audio_queue.clear();
        self.release_audio_decoder_only();
    }
}

/// Android ≥ 26 gate (AAudio + AImage + getOutputImage floor).
#[allow(dead_code)] // host builds never reach the android callers
pub fn sdk_gate() -> bool {
    #[cfg(target_os = "android")]
    {
        crate::platform::sdk_int().map(|i| i >= 26).unwrap_or(false)
    }
    #[cfg(not(target_os = "android"))]
    {
        false
    }
}

/// Mirror a drained video NAL to the DVR muxer (JNI → Kotlin LiveRecorder).
#[cfg(target_os = "android")]
pub fn dvr_write_video(
    nal: &[u8],
    is_key: bool,
    is_config: bool,
    width: i32,
    height: i32,
) -> Result<(), String> {
    crate::platform::live_recorder_write_video(nal, is_key, is_config, width, height)
}

#[cfg(not(target_os = "android"))]
#[allow(dead_code)]
pub fn dvr_write_video(_nal: &[u8], _k: bool, _c: bool, _w: i32, _h: i32) -> Result<(), String> {
    Err("off-Android".to_string())
}

/// Mirror a drained AAC blob to the DVR muxer (JNI → Kotlin LiveRecorder).
#[cfg(target_os = "android")]
pub fn dvr_write_audio(blob: &[u8], is_config: bool) -> Result<(), String> {
    crate::platform::live_recorder_write_audio(blob, is_config)
}

#[cfg(not(target_os = "android"))]
#[allow(dead_code)]
pub fn dvr_write_audio(_blob: &[u8], _c: bool) -> Result<(), String> {
    Err("off-Android".to_string())
}

/// Start the DVR. Returns the output path or None on failure.
pub fn dvr_start() -> Option<String> {
    #[cfg(target_os = "android")]
    {
        crate::platform::live_recorder_start().ok()
    }
    #[cfg(not(target_os = "android"))]
    {
        None
    }
}

/// Stop the DVR and seal the MP4. Returns the recorded file path (or None
/// when nothing was recorded).
pub fn dvr_stop() -> Option<String> {
    #[cfg(target_os = "android")]
    {
        crate::platform::live_recorder_stop().ok()
    }
    #[cfg(not(target_os = "android"))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_android_defaults() {
        if cfg!(target_os = "android") {
            return;
        }
        assert!(!h264::is_supported());
        assert!(!h264::init_encode(640, 480, 800_000, 15));
        assert!(h264::feed_encode(&[0u8; 4]).is_empty());
        assert!(!h264::init_decode());
        assert!(h264::feed_decode(&[0u8; 4]).is_empty());
        assert!(h264::release());
        assert!(!audio::is_supported());
        assert!(!audio::init_encode());
        assert!(audio::set_mic_enable(true));
        assert!(audio::drain().is_empty());
        assert!(!audio::init_decode());
        assert!(!audio::feed_aac(&[0u8; 4]));
        assert!(audio::release());
        assert!(dvr_start().is_none());
        assert!(dvr_stop().is_none());
    }
}
