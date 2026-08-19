//! FastCDC content-defined chunking + chunk manifests.
//!
//! Media blobs are sliced into variable-sized chunks at content-defined
//! boundaries (gear-hash cut points) instead of fixed byte offsets. Two blobs
//! that share content (same viral clip, same audio track, same intro frame)
//! produce identical chunk boundaries for the shared region — so identical
//! chunk data hashes to one BLAKE3 id and is stored in the CAS exactly once.
//!
//! Chunk sizing follows the schema used across the relay path (256 KiB
//! nominal) with FastCDC's min/avg/max window.

use fastcdc::v2020::StreamCDC;
use serde::{Deserialize, Serialize};

/// Absolute floor for a chunk size.
pub const MIN_CHUNK: usize = 64 * 1024;
/// Nominal chunk size FastCDC targets.
pub const AVG_CHUNK: usize = 256 * 1024;
/// Absolute ceiling for a chunk size.
pub const MAX_CHUNK: usize = 1024 * 1024;

/// Content-defined chunking parameters with min/avg/max bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkingParams {
    pub min: usize,
    pub avg: usize,
    pub max: usize,
}

impl Default for ChunkingParams {
    fn default() -> Self {
        Self {
            min: MIN_CHUNK,
            avg: AVG_CHUNK,
            max: MAX_CHUNK,
        }
    }
}

impl ChunkingParams {
    /// Content-aware chunking window tuned by MIME media type.
    pub fn for_mime(mime: &str) -> Self {
        if mime.starts_with("video/") {
            Self {
                min: 1024 * 1024,
                avg: 4 * 1024 * 1024,
                max: 16 * 1024 * 1024,
            }
        } else if mime.starts_with("audio/") {
            Self {
                min: 32 * 1024,
                avg: 128 * 1024,
                max: 512 * 1024,
            }
        } else {
            Self::default()
        }
    }
}

/// A single content-addressed chunk within a blob.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChunkRef {
    /// BLAKE3 hex digest of the chunk bytes (CAS key).
    pub blake3: String,
    /// Byte offset of this chunk within the source blob.
    pub offset: u64,
    /// Length of the chunk in bytes.
    pub len: usize,
}

/// Describes how a blob decomposes into content-addressed chunks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChunkManifest {
    /// BLAKE3 hex digest of the whole blob.
    pub blob_hash: String,
    /// Total blob size in bytes.
    pub total_size: u64,
    /// Chunks in ascending offset order (contiguous, cover the blob).
    pub chunks: Vec<ChunkRef>,
}

impl ChunkManifest {
    /// Validation for manifests received from peers: chunks must be sorted,
    /// contiguous, and the last chunk must exactly cover `total_size`. Rejects
    /// crafted manifests that would make a swarm downloader overrun the file.
    pub fn is_valid(&self) -> bool {
        if self.total_size == 0 {
            return self.chunks.is_empty();
        }
        let mut next = 0u64;
        for c in &self.chunks {
            if c.offset != next || c.len == 0 || c.blake3.len() != 64 {
                return false;
            }
            if !c.blake3.bytes().all(|b| b.is_ascii_hexdigit()) {
                return false;
            }
            next = c.offset.saturating_add(c.len as u64);
            if next > self.total_size {
                return false;
            }
        }
        next == self.total_size
    }
}

/// Chunk bytes paired with their references` — the type emitted by the
/// chunker so callers can store or re-encode without re-reading the source.
pub type ChunkedData = (ChunkManifest, Vec<(ChunkRef, Vec<u8>)>);

/// Chunks a reader into a manifest plus the chunk byte payloads using custom chunking parameters.
pub fn chunk_reader_with_data_params<R: std::io::Read>(
    reader: R,
    params: ChunkingParams,
) -> Result<ChunkedData, String> {
    let chunker = StreamCDC::new(
        reader,
        params.min as u32,
        params.avg as u32,
        params.max as u32,
    );

    let mut chunks: Vec<(ChunkRef, Vec<u8>)> = Vec::with_capacity(32);
    let mut manifest_chunks: Vec<ChunkRef> = Vec::with_capacity(32);
    let mut blob_hasher = blake3::Hasher::new();
    for item in chunker {
        let chunk = item.map_err(|e| format!("FastCDC chunking failed: {e}"))?;
        blob_hasher.update(&chunk.data);
        let chunk_hash = blake3::hash(&chunk.data).to_hex().to_string();
        let cref = ChunkRef {
            blake3: chunk_hash,
            offset: chunk.offset,
            len: chunk.data.len(),
        };
        manifest_chunks.push(cref.clone());
        chunks.push((cref, chunk.data));
    }

    let manifest = ChunkManifest {
        blob_hash: blob_hasher.finalize().to_hex().to_string(),
        total_size: chunks
            .last()
            .map(|(c, _)| c.offset + c.len as u64)
            .unwrap_or(0),
        chunks: manifest_chunks,
    };
    Ok((manifest, chunks))
}

/// Chunks a reader into a manifest plus the chunk byte payloads using default parameters.
pub fn chunk_reader_with_data<R: std::io::Read>(reader: R) -> Result<ChunkedData, String> {
    chunk_reader_with_data_params(reader, ChunkingParams::default())
}

/// Chunks a reader's bytes into a manifest only, streaming chunk boundaries without storing chunk byte vectors in RAM.
pub fn chunk_reader<R: std::io::Read>(reader: R) -> Result<ChunkManifest, String> {
    let chunker = StreamCDC::new(reader, MIN_CHUNK as u32, AVG_CHUNK as u32, MAX_CHUNK as u32);

    let mut chunks: Vec<ChunkRef> = Vec::with_capacity(32);
    let mut blob_hasher = blake3::Hasher::new();
    for item in chunker {
        let chunk = item.map_err(|e| format!("FastCDC chunking failed: {e}"))?;
        blob_hasher.update(&chunk.data);
        let chunk_hash = blake3::hash(&chunk.data).to_hex().to_string();
        chunks.push(ChunkRef {
            blake3: chunk_hash,
            offset: chunk.offset,
            len: chunk.data.len(),
        });
    }

    let total_size = chunks.last().map(|c| c.offset + c.len as u64).unwrap_or(0);
    Ok(ChunkManifest {
        blob_hash: blob_hasher.finalize().to_hex().to_string(),
        total_size,
        chunks,
    })
}

/// Chunks an in-memory byte slice. For small blobs and tests.
pub fn chunk_bytes(data: &[u8]) -> Result<ChunkManifest, String> {
    chunk_reader(std::io::Cursor::new(data))
}

/// Streams chunks from a reader to `write_chunk` one at a time, retaining only
/// the manifest (chunk byte payloads are dropped as soon as they are written).
pub fn store_reader_with_data<R, W>(reader: R, mut write_chunk: W) -> Result<ChunkManifest, String>
where
    R: std::io::Read,
    W: FnMut(&ChunkRef, &[u8]) -> Result<(), String>,
{
    let chunker = StreamCDC::new(reader, MIN_CHUNK as u32, AVG_CHUNK as u32, MAX_CHUNK as u32);

    let mut manifest_chunks: Vec<ChunkRef> = Vec::with_capacity(32);
    let mut blob_hasher = blake3::Hasher::new();
    for item in chunker {
        let chunk = item.map_err(|e| format!("FastCDC chunking failed: {e}"))?;
        blob_hasher.update(&chunk.data);
        let chunk_hash = blake3::hash(&chunk.data).to_hex().to_string();
        let cref = ChunkRef {
            blake3: chunk_hash,
            offset: chunk.offset,
            len: chunk.data.len(),
        };
        write_chunk(&cref, &chunk.data)?;
        manifest_chunks.push(cref);
    }

    let total_size = manifest_chunks
        .last()
        .map(|c| c.offset + c.len as u64)
        .unwrap_or(0);
    Ok(ChunkManifest {
        blob_hash: blob_hasher.finalize().to_hex().to_string(),
        total_size,
        chunks: manifest_chunks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_idempotent_boundaries_for_shared_prefix() {
        let a = vec![7u8; 5 * 1024 * 1024];
        let b = [a.clone(), vec![9u8; 1024 * 1024]].concat();
        let ma = chunk_bytes(&a).unwrap();
        let mb = chunk_bytes(&b).unwrap();
        assert!(ma.chunks.len() < 40);
        assert!(ma.chunks.len() >= 4);
        assert!(mb.blob_hash != ma.blob_hash);
        assert!(mb.total_size == b.len() as u64);
    }

    #[test]
    fn manifest_covers_entire_blob() {
        let data: Vec<u8> = (0..3 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
        let m = chunk_bytes(&data).unwrap();
        assert_eq!(m.total_size, data.len() as u64);
        let last = m.chunks.last().unwrap();
        assert_eq!(last.offset + last.len as u64, m.total_size);
        assert!(m.is_valid());
    }

    #[test]
    fn empty_blob_yields_empty_manifest() {
        let m = chunk_bytes(&[]).unwrap();
        assert_eq!(m.total_size, 0);
        assert!(m.chunks.is_empty());
        assert!(m.is_valid());
    }

    #[test]
    fn validates_rejects_gaps_and_overflow() {
        let bytes = vec![1u8; 300 * 1024];
        let mut m = chunk_bytes(&bytes).unwrap();
        m.chunks[0].len += 1;
        assert!(!m.is_valid());
        m.chunks[0].len -= 2;
        assert!(!m.is_valid());
    }

    #[test]
    fn identical_content_yields_identical_manifest() {
        let m1 = chunk_bytes(&vec![3u8; 1024 * 1024]).unwrap();
        let m2 = chunk_bytes(&vec![3u8; 1024 * 1024]).unwrap();
        assert_eq!(m1, m2);
    }

    #[test]
    fn store_reader_matches_chunk_reader_with_data() {
        let data: Vec<u8> = (0..2 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
        let (manifest, chunks) = chunk_reader_with_data(std::io::Cursor::new(&data)).unwrap();
        let mut stored: Vec<(ChunkRef, Vec<u8>)> = Vec::new();
        let manifest2 = store_reader_with_data(std::io::Cursor::new(&data), |cref, bytes| {
            stored.push((cref.clone(), bytes.to_vec()));
            Ok(())
        })
        .unwrap();
        assert_eq!(manifest, manifest2);
        assert_eq!(chunks, stored);
    }
}
