//! Raw NDK FFI declarations for Android media codecs (libmediandk) and
//! low-latency audio (libaaudio). Android-only; host builds use the stubs in
//! codecs/mod.rs. No crate dependencies — direct #[link] against the NDK
//! shared libraries (both present since API 26, app minSdk gate is 26).

#![cfg(target_os = "android")]
#![allow(
    unsafe_code,
    non_camel_case_types,
    non_snake_case,
    dead_code,
    non_upper_case_globals
)]

use std::os::raw::{c_char, c_void};

pub type mediastatus_t = i32;
pub type ssize_t = isize;
pub type aaudio_result_t = i32;

pub const AMEDIACODEC_INFO_TRY_AGAIN_LATER: ssize_t = -1;
pub const AMEDIACODEC_INFO_OUTPUT_FORMAT_CHANGED: ssize_t = -2;
pub const AMEDIACODEC_INFO_OUTPUT_BUFFERS_CHANGED: ssize_t = -3;

pub const AMEDIACODEC_BUFFER_FLAG_CODEC_CONFIG: u32 = 2;
pub const AMEDIACODEC_BUFFER_FLAG_KEY_FRAME: u32 = 1;
pub const AMEDIACODEC_BUFFER_FLAG_END_OF_STREAM: u32 = 4;

pub const AMEDIACODEC_CONFIGURE_FLAG_ENCODE: u32 = 1;

pub const COLOR_FormatYUV420Flexible: i32 = 0x7F420888;

pub const AAUDIO_OK: aaudio_result_t = 0;
pub const AAUDIO_DIRECTION_OUTPUT: i32 = 0;
pub const AAUDIO_DIRECTION_INPUT: i32 = 1;
pub const AAUDIO_FORMAT_PCM_I16: i32 = 2;
pub const AAUDIO_INPUT_PRESET_VOICE_RECOGNITION: i32 = 6;
pub const AAUDIO_PERFORMANCE_MODE_LOW_LATENCY: i32 = 2;

pub enum AMediaCodec {}
pub enum AMediaFormat {}
pub enum AImage {}
pub enum AAudioStream {}
pub enum AAudioStreamBuilder {}

#[repr(C)]
pub struct AMediaCodecBufferInfo {
    pub offset: i32,
    pub size: i32,
    pub presentation_time_us: i64,
    pub flags: u32,
}

#[link(name = "mediandk")]
extern "C" {
    pub fn AMediaCodec_createEncoderByType(mime: *const c_char) -> *mut AMediaCodec;
    pub fn AMediaCodec_createDecoderByType(mime: *const c_char) -> *mut AMediaCodec;
    pub fn AMediaCodec_delete(codec: *mut AMediaCodec) -> mediastatus_t;
    pub fn AMediaCodec_configure(
        codec: *mut AMediaCodec,
        format: *const AMediaFormat,
        surface: *mut c_void,
        crypto: *mut c_void,
        flags: u32,
    ) -> mediastatus_t;
    pub fn AMediaCodec_start(codec: *mut AMediaCodec) -> mediastatus_t;
    pub fn AMediaCodec_stop(codec: *mut AMediaCodec) -> mediastatus_t;
    pub fn AMediaCodec_dequeueInputBuffer(codec: *mut AMediaCodec, timeout_us: i64) -> ssize_t;
    pub fn AMediaCodec_queueInputBuffer(
        codec: *mut AMediaCodec,
        idx: usize,
        offset: usize,
        size: usize,
        timestamp_us: u64,
        flags: u32,
    ) -> mediastatus_t;
    pub fn AMediaCodec_dequeueOutputBuffer(
        codec: *mut AMediaCodec,
        info: *mut AMediaCodecBufferInfo,
        timeout_us: i64,
    ) -> ssize_t;
    pub fn AMediaCodec_getInputBuffer(
        codec: *mut AMediaCodec,
        idx: usize,
        out_size: *mut usize,
    ) -> *mut u8;
    pub fn AMediaCodec_getOutputBuffer(
        codec: *mut AMediaCodec,
        idx: usize,
        out_size: *mut usize,
    ) -> *const u8;
    pub fn AMediaCodec_releaseOutputBuffer(
        codec: *mut AMediaCodec,
        idx: usize,
        render: u8, // C _Bool
    ) -> mediastatus_t;
    pub fn AMediaCodec_getOutputImage(
        codec: *mut AMediaCodec,
        idx: usize,
        image: *mut *mut AImage,
    ) -> mediastatus_t;
    pub fn AMediaCodec_getOutputFormat(codec: *mut AMediaCodec) -> *mut AMediaFormat;
    pub fn AMediaCodec_getInputFormat(codec: *mut AMediaCodec) -> *mut AMediaFormat;

    pub fn AMediaFormat_new() -> *mut AMediaFormat;
    pub fn AMediaFormat_delete(format: *mut AMediaFormat);
    pub fn AMediaFormat_setString(
        format: *mut AMediaFormat,
        key: *const c_char,
        value: *const c_char,
    );
    pub fn AMediaFormat_setInt32(format: *mut AMediaFormat, key: *const c_char, value: i32);
    pub fn AMediaFormat_getInt32(
        format: *const AMediaFormat,
        key: *const c_char,
        out: *mut i32,
    ) -> bool;

    pub fn AImage_release(image: *mut AImage);
    pub fn AImage_getWidth(image: *const AImage, width: *mut i32) -> mediastatus_t;
    pub fn AImage_getHeight(image: *const AImage, height: *mut i32) -> mediastatus_t;
    pub fn AImage_getNumberOfPlanes(image: *const AImage, num_planes: *mut i32) -> mediastatus_t;
    pub fn AImage_getPlaneData(
        image: *const AImage,
        plane_idx: i32,
        data: *mut *mut u8,
        data_length: *mut i32,
    ) -> mediastatus_t;
    pub fn AImage_getPlaneRowStride(
        image: *const AImage,
        plane_idx: i32,
        row_stride: *mut i32,
    ) -> mediastatus_t;
    pub fn AImage_getPlanePixelStride(
        image: *const AImage,
        plane_idx: i32,
        pixel_stride: *mut i32,
    ) -> mediastatus_t;
}

#[link(name = "aaudio")]
extern "C" {
    pub fn AAudioStreamBuilder_create(builder: *mut *mut AAudioStreamBuilder) -> aaudio_result_t;
    pub fn AAudioStreamBuilder_delete(builder: *mut AAudioStreamBuilder);
    pub fn AAudioStreamBuilder_setDirection(builder: *mut AAudioStreamBuilder, direction: i32);
    pub fn AAudioStreamBuilder_setFormat(builder: *mut AAudioStreamBuilder, format: i32);
    pub fn AAudioStreamBuilder_setSampleRate(builder: *mut AAudioStreamBuilder, sample_rate: i32);
    pub fn AAudioStreamBuilder_setChannelCount(
        builder: *mut AAudioStreamBuilder,
        channel_count: i32,
    );
    pub fn AAudioStreamBuilder_setInputPreset(builder: *mut AAudioStreamBuilder, preset: i32);
    pub fn AAudioStreamBuilder_setPerformanceMode(builder: *mut AAudioStreamBuilder, mode: i32);
    pub fn AAudioStreamBuilder_setBufferCapacityInFrames(
        builder: *mut AAudioStreamBuilder,
        frames: i32,
    );
    pub fn AAudioStreamBuilder_openStream(
        builder: *mut AAudioStreamBuilder,
        stream: *mut *mut AAudioStream,
    ) -> aaudio_result_t;
    pub fn AAudioStream_requestStart(stream: *mut AAudioStream) -> aaudio_result_t;
    pub fn AAudioStream_requestStop(stream: *mut AAudioStream) -> aaudio_result_t;
    pub fn AAudioStream_read(
        stream: *mut AAudioStream,
        buffer: *mut c_void,
        num_frames: i32,
        timeout_nanoseconds: i64,
    ) -> aaudio_result_t;
    pub fn AAudioStream_write(
        stream: *mut AAudioStream,
        buffer: *const c_void,
        num_frames: i32,
        timeout_nanoseconds: i64,
    ) -> aaudio_result_t;
    pub fn AAudioStream_close(stream: *mut AAudioStream) -> aaudio_result_t;
}
