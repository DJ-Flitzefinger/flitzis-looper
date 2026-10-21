//! Fixed callback-owned mode acknowledgement and actual wet readiness.
use std::sync::atomic::{AtomicU8, AtomicU64, AtomicUsize, Ordering};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct KeyLockStatus {
    pub(crate) source_generation: u64,
    pub(crate) source_address: usize,
    pub(crate) window_revision: u64,
    pub(crate) request_id: u64,
    pub(crate) effective: bool,
    pub(crate) ready: bool,
    pub(crate) state: &'static str,
    pub(crate) error: Option<&'static str>,
}

/// Fixed witness prepared from the original admitted map before entering realtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SourceSyncWitness {
    pub(crate) intent_revision: u64,
    pub(crate) map_revision: [u64; 4],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SourceSyncStatus {
    pub(crate) content_lifetime: u64,
    pub(crate) source_generation: u64,
    pub(crate) source_address: usize,
    pub(crate) window_revision: u64,
    pub(crate) active: bool,
    pub(crate) witness: Option<SourceSyncWitness>,
}

#[derive(Default)]
pub(super) struct KeyLockStatusSlot {
    pub(super) request: AtomicU64,
    sequence: AtomicU64,
    generation: AtomicU64,
    address: AtomicUsize,
    window: AtomicU64,
    acknowledged_request: AtomicU64,
    flags: AtomicU8,
    sync_generation: AtomicU64,
    sync_lifetime: AtomicU64,
    sync_address: AtomicUsize,
    sync_window: AtomicU64,
    sync_flags: AtomicU8,
    sync_intent: AtomicU64,
    sync_map: [AtomicU64; 4],
}

impl KeyLockStatusSlot {
    pub(super) fn clear(&self) {
        self.generation.store(0, Ordering::SeqCst);
        self.sync_generation.store(0, Ordering::SeqCst);
    }

    pub(super) fn publish_source_sync(&self, status: SourceSyncStatus) {
        // Same sole callback writer and sequence as mode feedback. SYNC cannot
        // depend on a KEYLOCK request ACK: a pending mode request may coexist
        // with an old mapped voice, stop, or a replacement source.
        let sequence = self.sequence.load(Ordering::SeqCst);
        self.sequence
            .store(sequence.wrapping_add(1), Ordering::SeqCst);
        self.sync_generation
            .store(status.source_generation, Ordering::SeqCst);
        self.sync_lifetime
            .store(status.content_lifetime, Ordering::SeqCst);
        self.sync_address
            .store(status.source_address, Ordering::SeqCst);
        self.sync_window
            .store(status.window_revision, Ordering::SeqCst);
        self.sync_flags.store(
            u8::from(status.active) | (u8::from(status.witness.is_some()) << 1),
            Ordering::SeqCst,
        );
        let witness = status.witness.unwrap_or(SourceSyncWitness {
            intent_revision: 0,
            map_revision: [0; 4],
        });
        self.sync_intent
            .store(witness.intent_revision, Ordering::SeqCst);
        for (word, value) in self.sync_map.iter().zip(witness.map_revision) {
            word.store(value, Ordering::SeqCst);
        }
        self.sequence
            .store(sequence.wrapping_add(2), Ordering::SeqCst);
    }

    pub(super) fn read_source_sync(&self) -> Option<SourceSyncStatus> {
        let sequence = self.sequence.load(Ordering::SeqCst);
        if sequence & 1 != 0 {
            return None;
        }
        let flags = self.sync_flags.load(Ordering::SeqCst);
        let status = SourceSyncStatus {
            content_lifetime: self.sync_lifetime.load(Ordering::SeqCst),
            source_generation: self.sync_generation.load(Ordering::SeqCst),
            source_address: self.sync_address.load(Ordering::SeqCst),
            window_revision: self.sync_window.load(Ordering::SeqCst),
            active: flags & 1 != 0,
            witness: (flags & 2 != 0).then(|| SourceSyncWitness {
                intent_revision: self.sync_intent.load(Ordering::SeqCst),
                map_revision: std::array::from_fn(|index| {
                    self.sync_map[index].load(Ordering::SeqCst)
                }),
            }),
        };
        (status.source_generation != 0 && self.sequence.load(Ordering::SeqCst) == sequence)
            .then_some(status)
    }

    pub(super) fn publish(&self, status: KeyLockStatus) {
        // One callback writer. Readers never spin and reject an overlapping write.
        let sequence = self.sequence.load(Ordering::SeqCst);
        self.sequence
            .store(sequence.wrapping_add(1), Ordering::SeqCst);
        self.generation
            .store(status.source_generation, Ordering::SeqCst);
        self.address.store(status.source_address, Ordering::SeqCst);
        self.window.store(status.window_revision, Ordering::SeqCst);
        self.acknowledged_request
            .store(status.request_id, Ordering::SeqCst);
        let state = match status.state {
            "armed" => 0,
            "dry" => 1,
            "wet" => 2,
            "waiting" => 3,
            _ => 4,
        };
        self.flags.store(
            state | (u8::from(status.effective) << 3) | (u8::from(status.ready) << 4),
            Ordering::SeqCst,
        );
        self.sequence
            .store(sequence.wrapping_add(2), Ordering::SeqCst);
    }

    pub(super) fn read(&self) -> Option<KeyLockStatus> {
        let sequence = self.sequence.load(Ordering::SeqCst);
        if sequence & 1 != 0 {
            return None;
        }
        let flags = self.flags.load(Ordering::SeqCst);
        let state = match flags & 7 {
            0 => "armed",
            1 => "dry",
            2 => "wet",
            3 => "waiting",
            _ => "error",
        };
        let status = KeyLockStatus {
            source_generation: self.generation.load(Ordering::SeqCst),
            source_address: self.address.load(Ordering::SeqCst),
            window_revision: self.window.load(Ordering::SeqCst),
            request_id: self.acknowledged_request.load(Ordering::SeqCst),
            effective: flags & 8 != 0,
            ready: flags & 16 != 0,
            state,
            error: (state == "error").then_some("Key Lock native processing is unavailable"),
        };
        (status.source_generation != 0 && self.sequence.load(Ordering::SeqCst) == sequence)
            .then_some(status)
    }
}
