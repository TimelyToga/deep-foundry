//! Shared types for all Deep Foundry crates.
//!
//! This crate is the contract between the simulation, the renderer, the UI and the game program.
//! Keep it small. Change it only through `docs/design/interface-requests.md`.

pub mod command;
pub mod consts;
pub mod ids;
pub mod mailbox;
pub mod pos;
pub mod rng;
pub mod snapshot;

pub use command::{Command, PaintMode};
pub use consts::*;
pub use ids::{BuildingId, BuildingKindId, MaterialId, PartId, RecipeId, TechId};
pub use mailbox::SnapshotMailbox;
pub use pos::{CellPos, CellRect, ChunkPos, TilePos, local_index, local_xy};
pub use rng::Rng;
pub use snapshot::{CellTexel, ChunkImage, DebugChunk, ParticleView, SimStats, Snapshot, pack_texel};
