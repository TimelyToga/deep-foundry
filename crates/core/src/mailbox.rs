//! A one-slot mailbox for snapshots. The simulation thread publishes. The main thread takes.
//! Neither side waits for the other for more than a very short lock.

use crate::snapshot::Snapshot;
use std::sync::Mutex;

#[derive(Default)]
pub struct SnapshotMailbox {
    slot: Mutex<Option<Snapshot>>,
}

impl SnapshotMailbox {
    pub fn new() -> Self {
        Self::default()
    }

    /// Put a new snapshot in the slot. If the reader did not take the previous one,
    /// merge it in, so no chunk update is lost. Only one thread may publish.
    pub fn publish(&self, mut snapshot: Snapshot) {
        let previous = self.slot.lock().unwrap().take();
        if let Some(previous) = previous {
            snapshot.merge_older(previous);
        }
        *self.slot.lock().unwrap() = Some(snapshot);
    }

    /// Take the newest snapshot, if there is one.
    pub fn take(&self) -> Option<Snapshot> {
        self.slot.lock().unwrap().take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ChunkImage, ChunkPos};

    #[test]
    fn publish_twice_keeps_all_chunks() {
        let mb = SnapshotMailbox::new();
        let mut a = Snapshot { tick: 1, ..Default::default() };
        a.chunks.push(ChunkImage::new_air(ChunkPos::new(0, 0)));
        a.chunks.push(ChunkImage::new_air(ChunkPos::new(1, 0)));
        let mut b = Snapshot { tick: 2, ..Default::default() };
        b.chunks.push(ChunkImage::new_air(ChunkPos::new(1, 0)));
        mb.publish(a);
        mb.publish(b);
        let s = mb.take().unwrap();
        assert_eq!(s.tick, 2);
        assert_eq!(s.chunks.len(), 2);
        assert!(mb.take().is_none());
    }
}
