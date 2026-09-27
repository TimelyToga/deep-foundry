# Interface requests from the render-game task

## 2026-09-27 render-game — reuse chunk image memory

What: a way to give used `ChunkImage` buffers back to the simulation thread, so that
`Simulation::take_snapshot` can fill old buffers instead of making a new 32 KiB `Box<[CellTexel]>`
for each changed chunk in each tick. For example, `SnapshotMailbox` could have a second slot or a
channel for used images: the main thread puts them there after `Renderer::apply_snapshot`, and the
simulation takes them when it packs chunks.

Why: technical design section 12 says no large buffer allocations per frame. Today each tick makes
one heap allocation per changed chunk in view (and the main thread frees it). With many moving
chunks this is up to about 1,000 allocations of 32 KiB per tick.

Stub until then: none needed. The game drops the snapshot after the upload, as now. The renderer
already copies the data into reused GPU staging memory (`wgpu::util::StagingBelt`), so only the
simulation side allocates.

## 2026-09-27 render-game — (later) changed rows per chunk

What: optional per-chunk dirty row range in `ChunkImage` (for example `rows: Range<u8>`), so the
renderer can upload only the rows that changed.

Why: an upload now costs about 1.7 µs per chunk (32 KiB, measured on the M1 Max), so it is not
needed yet. It would help when very many chunks change a few cells each tick.

Stub until then: the renderer uploads whole chunks.
