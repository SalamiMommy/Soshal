use base64::{engine::general_purpose, Engine as _};
use flate2::Compression;
use std::io::{Cursor, Read, Write};

/// zstd dictionary trained over feed/post JSON payloads
/// (see scripts/train-zstd-dict.py). Bundled with the app binary.
const FEED_DICT: &[u8] = include_bytes!("../assets/feed.dict");

/// Frame magic: `ZH` + version + dict id (u32 LE). Dict id lets the server
/// negotiate: a mismatched id means "retrain / fall back to flate2".
pub const ZSTD_DICT_MAGIC: [u8; 6] = [0x5A, 0x48, 0x01, 0x00, 0x00, 0x00];
pub const ZSTD_DICT_ID: u32 = 1;
/// Compressed-dict frame header: [`ZSTD_DICT_MAGIC`] + id (LE u32).
pub const ZSTD_DICT_HEADER_LEN: usize = ZSTD_DICT_MAGIC.len() + 4;
const ZSTD_DICT_LEVEL: i32 = 19;

pub fn compress(data: &[u8]) -> Result<Vec<u8>, String> {
    let cap = (data.len() / 2).max(128);
    let mut encoder =
        flate2::write::DeflateEncoder::new(Vec::with_capacity(cap), Compression::default());
    encoder
        .write_all(data)
        .map_err(|e| format!("compress: {}", e))?;
    encoder
        .finish()
        .map_err(|e| format!("compress finish: {}", e))
}

/// Default cap for decompressed output (4 MiB) — remote-compressed payloads
/// must never inflate beyond this.
pub const MAX_DECOMPRESS_BYTES: usize = 4 * 1024 * 1024;

pub fn decompress(data: &[u8]) -> Result<Vec<u8>, String> {
    decompress_limited(data, MAX_DECOMPRESS_BYTES)
}

/// Decompresses with a hard output cap; aborts as soon as the limit is
/// exceeded, before allocating the remaining output.
pub fn decompress_limited(data: &[u8], max_bytes: usize) -> Result<Vec<u8>, String> {
    let mut decoder = flate2::write::DeflateDecoder::new(Vec::new());
    for chunk in data.chunks(64 * 1024) {
        decoder
            .write_all(chunk)
            .map_err(|e| format!("decompress: {}", e))?;
        if decoder.total_out() as usize > max_bytes {
            return Err(format!("decompressed payload exceeds {max_bytes} bytes"));
        }
    }
    let out = decoder
        .finish()
        .map_err(|e| format!("decompress finish: {}", e))?;
    if out.len() > max_bytes {
        return Err(format!("decompressed payload exceeds {max_bytes} bytes"));
    }
    Ok(out)
}

pub fn compress_json(data: &str) -> String {
    match compress(data.as_bytes()) {
        Ok(compressed) => general_purpose::STANDARD.encode(&compressed),
        Err(_) => String::new(),
    }
}

pub fn decompress_json(encoded: &str) -> String {
    match general_purpose::STANDARD.decode(encoded) {
        Ok(bytes) => match decompress(&bytes) {
            Ok(decompressed) => String::from_utf8(decompressed).unwrap_or_default(),
            Err(_) => String::new(),
        },
        Err(_) => String::new(),
    }
}

/// True when the payload carries a zstd-dict frame (magic + known dict id).
pub fn is_dict_frame(data: &[u8]) -> bool {
    data.len() > ZSTD_DICT_MAGIC.len() && data[..ZSTD_DICT_MAGIC.len()] == ZSTD_DICT_MAGIC
}

/// Compresses with the bundled trained dictionary. Output = magic || zstd
/// frame. Up to ~70% smaller than deflate on small repetitive JSON.
pub fn compress_dict(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut encoder =
        zstd::stream::Encoder::with_dictionary(Vec::new(), ZSTD_DICT_LEVEL, FEED_DICT)
            .map_err(|e| format!("compress_dict init: {}", e))?;
    encoder
        .write_all(data)
        .map_err(|e| format!("compress_dict: {}", e))?;
    let frame = encoder
        .finish()
        .map_err(|e| format!("compress_dict finish: {}", e))?;
    let mut out = Vec::with_capacity(ZSTD_DICT_MAGIC.len() + frame.len());
    out.extend_from_slice(&ZSTD_DICT_MAGIC);
    out.extend_from_slice(&ZSTD_DICT_ID.to_le_bytes());
    out.extend_from_slice(&frame);
    Ok(out)
}

/// Decompresses a zstd-dict frame; rejects non-dict input and non-matching
/// dict ids (caller should fall back to `decompress`).
pub fn decompress_dict(data: &[u8]) -> Result<Vec<u8>, String> {
    decompress_dict_limited(data, MAX_DECOMPRESS_BYTES)
}

pub fn decompress_dict_limited(data: &[u8], max_bytes: usize) -> Result<Vec<u8>, String> {
    if !is_dict_frame(data) {
        return Err("not a zstd-dict frame".to_string());
    }
    let id_bytes: [u8; 4] = data[ZSTD_DICT_MAGIC.len()..ZSTD_DICT_MAGIC.len() + 4]
        .try_into()
        .map_err(|_| "truncated dict id".to_string())?;
    if u32::from_le_bytes(id_bytes) != ZSTD_DICT_ID {
        return Err(format!("dict id mismatch: expected {ZSTD_DICT_ID}"));
    }
    let frame = &data[ZSTD_DICT_MAGIC.len() + 4..];
    let decoder = zstd::stream::read::Decoder::with_dictionary(Cursor::new(frame), FEED_DICT)
        .map_err(|e| format!("decompress_dict init: {}", e))?;
    let mut out = Vec::new();
    let mut limited = decoder.take(max_bytes as u64 + 1);
    limited
        .read_to_end(&mut out)
        .map_err(|e| format!("decompress_dict: {}", e))?;
    if out.len() > max_bytes {
        return Err(format!("decompressed payload exceeds {max_bytes} bytes"));
    }
    Ok(out)
}

/// Dictionary variant of `compress_json`; empty string on failure.
pub fn compress_json_dict(data: &str) -> String {
    match compress_dict(data.as_bytes()) {
        Ok(compressed) => general_purpose::STANDARD.encode(&compressed),
        Err(_) => String::new(),
    }
}

/// Dictionary variant of `decompress_json`; falls back to plain deflate
/// when the payload is not a dict frame.
pub fn decompress_json_dict(encoded: &str) -> String {
    let Ok(bytes) = general_purpose::STANDARD.decode(encoded) else {
        return String::new();
    };
    if is_dict_frame(&bytes) {
        match decompress_dict(&bytes) {
            Ok(decompressed) => String::from_utf8(decompressed).unwrap_or_default(),
            Err(_) => String::new(),
        }
    } else {
        decompress_json(encoded)
    }
}
