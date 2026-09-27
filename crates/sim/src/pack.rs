//! Packed chunks: the cell arrays of a chunk, compressed with lz4.
//!
//! The world packs a changed chunk when it is asleep and far from every anchor, and unpacks it
//! when it is needed again. Save files use the same bytes, so saving a packed chunk needs no work.
//!
//! `ChunkStore` is the place for a later disk store (region files). See its documentation.

use crate::chunk::{Chunk, LocalRect};
use foundry_core::{CHUNK_AREA, ChunkPos, MaterialId};

/// Bytes of one chunk's cell arrays: mat, temp (2 bytes each), shade, life, motion, flags.
pub const RAW_CHUNK: usize = CHUNK_AREA * 8;

/// A chunk with its cell arrays compressed.
#[derive(Clone, PartialEq, Eq)]
pub struct PackedChunk {
    /// An lz4 block (with the raw size in front, see `lz4_flex::compress_prepend_size`) of the
    /// cell arrays in the layout of `encode`.
    pub bytes: Box<[u8]>,
    /// The cells to update when the chunk is unpacked.
    pub dirty: LocalRect,
    pub version: u64,
    /// The cells are as the chunk source made them. Such a chunk is only packed (and saved) when it
    /// has work (a non-empty `dirty`); otherwise the world drops it.
    pub pristine: bool,
}

impl std::fmt::Debug for PackedChunk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PackedChunk").field("bytes", &self.bytes.len()).field("dirty", &self.dirty).finish()
    }
}

impl PackedChunk {
    pub fn pack(c: &Chunk) -> PackedChunk {
        let mut raw = vec![0u8; RAW_CHUNK];
        encode(c, &mut raw);
        PackedChunk {
            bytes: lz4_flex::compress_prepend_size(&raw).into_boxed_slice(),
            dirty: c.dirty,
            version: c.version,
            pristine: c.pristine,
        }
    }

    /// The chunk again. `remap` changes saved material ids into current ones (for loading); `None` keeps them.
    pub fn unpack(&self, remap: Option<&[MaterialId]>) -> Result<Box<Chunk>, String> {
        let raw = lz4_flex::decompress_size_prepended(&self.bytes).map_err(|e| e.to_string())?;
        if raw.len() != RAW_CHUNK {
            return Err(format!("{} bytes of cells, expected {RAW_CHUNK}", raw.len()));
        }
        let mut c = Chunk::new_air();
        decode(&raw, &mut c, remap);
        c.dirty = self.dirty;
        c.version = self.version;
        c.pristine = self.pristine;
        Ok(c)
    }
}

/// Write the cell arrays of a chunk into `out` (`RAW_CHUNK` bytes, little-endian).
pub fn encode(c: &Chunk, out: &mut [u8]) {
    let (mat, rest) = out.split_at_mut(CHUNK_AREA * 2);
    let (temp, rest) = rest.split_at_mut(CHUNK_AREA * 2);
    for i in 0..CHUNK_AREA {
        mat[i * 2..i * 2 + 2].copy_from_slice(&c.mat[i].to_le_bytes());
        temp[i * 2..i * 2 + 2].copy_from_slice(&c.temp[i].to_le_bytes());
    }
    rest[..CHUNK_AREA].copy_from_slice(&c.shade);
    rest[CHUNK_AREA..CHUNK_AREA * 2].copy_from_slice(&c.life);
    rest[CHUNK_AREA * 2..CHUNK_AREA * 3].copy_from_slice(&c.motion);
    rest[CHUNK_AREA * 3..].copy_from_slice(&c.flags);
}

/// Read the cell arrays written by `encode`. `remap` changes material ids (unknown ids become air).
pub fn decode(raw: &[u8], c: &mut Chunk, remap: Option<&[MaterialId]>) {
    let (mat, rest) = raw.split_at(CHUNK_AREA * 2);
    let (temp, rest) = rest.split_at(CHUNK_AREA * 2);
    for i in 0..CHUNK_AREA {
        let saved = u16::from_le_bytes([mat[i * 2], mat[i * 2 + 1]]);
        c.mat[i] = match remap {
            Some(r) => r.get(saved as usize).copied().unwrap_or(MaterialId::AIR).0,
            None => saved,
        };
        c.temp[i] = i16::from_le_bytes([temp[i * 2], temp[i * 2 + 1]]);
    }
    c.shade.copy_from_slice(&rest[..CHUNK_AREA]);
    c.life.copy_from_slice(&rest[CHUNK_AREA..CHUNK_AREA * 2]);
    c.motion.copy_from_slice(&rest[CHUNK_AREA * 2..CHUNK_AREA * 3]);
    c.flags.copy_from_slice(&rest[CHUNK_AREA * 3..]);
}

/// A place for packed chunks outside memory, for example region files on disk.
///
/// This is a hook for later. By default a world has no store and keeps all packed chunks in
/// memory. With a store (`World::set_store`), the world moves packed chunks into it when the
/// packed chunks in memory use more than the limit, farthest from the anchors first, and takes
/// them back when it needs them.
///
/// A disk store must also keep an index in memory, so that `contains` and `positions` are fast.
pub trait ChunkStore: Send {
    /// Keep this chunk. The world forgets it.
    fn put(&mut self, pos: ChunkPos, chunk: PackedChunk);
    /// Give back the chunk and forget it. `None` if the store does not have it.
    fn take(&mut self, pos: ChunkPos) -> Option<PackedChunk>;
    /// A copy of the chunk; the store keeps it. For saves and for reading single cells.
    fn read(&self, pos: ChunkPos) -> Option<PackedChunk>;
    fn contains(&self, pos: ChunkPos) -> bool;
    /// The positions of all chunks in the store, in any order.
    fn positions(&self) -> Vec<ChunkPos>;
}
