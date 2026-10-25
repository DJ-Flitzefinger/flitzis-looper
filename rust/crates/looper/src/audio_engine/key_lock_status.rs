//! Fixed callback-owned mode acknowledgement and actual wet readiness.
use crate::audio_engine::key_lock_start_policy::KeyLockStartBehavior;
use crate::audio_engine::mapped_birth::BirthStatus;
use crate::audio_engine::source_sync_policy::{
    SourceSyncMode, SourceSyncObservation, SourceSyncRequest,
};
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
    pub(crate) policy: Option<SourceSyncObservation>,
    pub(crate) birth: Option<BirthStatus>,
    pub(crate) birth_source_request_incarnation: Option<u64>,
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
    sync_policy_mode: AtomicU8,
    sync_policy_revision: AtomicU64,
    sync_policy_choices: AtomicU8,
    sync_phase_revision: AtomicU64,
    sync_birth_flags: AtomicU8,
    sync_birth_original: AtomicU64,
    sync_birth_execution: AtomicU64,
    sync_birth_source_request: AtomicU64,
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
            u8::from(status.active)
                | (u8::from(status.witness.is_some()) << 1)
                | (u8::from(status.policy.is_some()) << 2)
                | (u8::from(
                    status
                        .policy
                        .is_some_and(|policy| policy.request.global_revision.is_some()),
                ) << 3)
                | (u8::from(status.policy.is_some_and(|policy| policy.settled)) << 4)
                | (u8::from(status.policy.is_some_and(|policy| policy.target_current)) << 5)
                | (u8::from(
                    status
                        .policy
                        .is_some_and(|policy| policy.phase_program_current),
                ) << 6),
            Ordering::SeqCst,
        );
        let witness = status.witness.unwrap_or(SourceSyncWitness {
            intent_revision: 0,
            map_revision: [0; 4],
        });
        self.sync_intent.store(
            status.policy.map_or(witness.intent_revision, |policy| {
                policy.request.intent_revision
            }),
            Ordering::SeqCst,
        );
        self.sync_policy_mode.store(
            status.policy.map_or(0, |policy| match policy.request.mode {
                SourceSyncMode::Legacy => 0,
                SourceSyncMode::Off => 1,
                SourceSyncMode::Fixed => 2,
                SourceSyncMode::Mapped => 3,
            }),
            Ordering::SeqCst,
        );
        self.sync_policy_revision.store(
            status
                .policy
                .and_then(|policy| policy.request.global_revision)
                .unwrap_or(0),
            Ordering::SeqCst,
        );
        let choice = |value: Option<bool>| match value {
            None => 0,
            Some(false) => 1,
            Some(true) => 2,
        };
        self.sync_policy_choices.store(
            status.policy.map_or(0, |policy| {
                choice(policy.request.policy_sync)
                    | (choice(policy.request.policy_realtime_bpm) << 2)
            }),
            Ordering::SeqCst,
        );
        self.sync_phase_revision.store(
            status
                .policy
                .and_then(|policy| policy.phase_program_revision)
                .unwrap_or(0),
            Ordering::SeqCst,
        );
        for (word, value) in self.sync_map.iter().zip(witness.map_revision) {
            word.store(value, Ordering::SeqCst);
        }
        self.sync_birth_source_request.store(
            status.birth_source_request_incarnation.unwrap_or(0),
            Ordering::SeqCst,
        );
        self.sync_birth_flags.store(
            status.birth.map_or(0, |birth| {
                1 | ((birth.requested as u8) << 1)
                    | (u8::from(birth.mixed) << 2)
                    | (u8::from(birth.reference_qualified) << 3)
                    | (u8::from(birth.emitted) << 4)
                    | (u8::from(birth.original.is_some()) << 5)
                    | (u8::from(birth.execution.is_some()) << 6)
            }),
            Ordering::SeqCst,
        );
        self.sync_birth_original.store(
            status.birth.and_then(|birth| birth.original).unwrap_or(0),
            Ordering::SeqCst,
        );
        self.sync_birth_execution.store(
            status.birth.and_then(|birth| birth.execution).unwrap_or(0),
            Ordering::SeqCst,
        );
        self.sequence
            .store(sequence.wrapping_add(2), Ordering::SeqCst);
    }

    pub(super) fn read_source_sync(&self) -> Option<SourceSyncStatus> {
        let sequence = self.sequence.load(Ordering::SeqCst);
        if sequence & 1 != 0 {
            return None;
        }
        let flags = self.sync_flags.load(Ordering::SeqCst);
        let birth_flags = self.sync_birth_flags.load(Ordering::SeqCst);
        let status = SourceSyncStatus {
            birth_source_request_incarnation: (birth_flags & 1 != 0)
                .then(|| self.sync_birth_source_request.load(Ordering::SeqCst)),
            birth: (birth_flags & 1 != 0).then(|| BirthStatus {
                requested: if birth_flags & 2 != 0 {
                    KeyLockStartBehavior::AttackAtGrid
                } else {
                    KeyLockStartBehavior::FirstSoundAtGrid
                },
                mixed: birth_flags & 4 != 0,
                reference_qualified: birth_flags & 8 != 0,
                emitted: birth_flags & 16 != 0,
                original: (birth_flags & 32 != 0)
                    .then(|| self.sync_birth_original.load(Ordering::SeqCst)),
                execution: (birth_flags & 64 != 0)
                    .then(|| self.sync_birth_execution.load(Ordering::SeqCst)),
            }),
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
            policy: (flags & 4 != 0).then(|| SourceSyncObservation {
                request: SourceSyncRequest {
                    mode: match self.sync_policy_mode.load(Ordering::SeqCst) {
                        1 => SourceSyncMode::Off,
                        2 => SourceSyncMode::Fixed,
                        3 => SourceSyncMode::Mapped,
                        _ => SourceSyncMode::Legacy,
                    },
                    global_revision: (flags & 8 != 0)
                        .then(|| self.sync_policy_revision.load(Ordering::SeqCst)),
                    intent_revision: self.sync_intent.load(Ordering::SeqCst),
                    policy_sync: match self.sync_policy_choices.load(Ordering::SeqCst) & 3 {
                        1 => Some(false),
                        2 => Some(true),
                        _ => None,
                    },
                    policy_realtime_bpm: match (self.sync_policy_choices.load(Ordering::SeqCst)
                        >> 2)
                        & 3
                    {
                        1 => Some(false),
                        2 => Some(true),
                        _ => None,
                    },
                },
                settled: flags & 16 != 0,
                target_current: flags & 32 != 0,
                phase_program_current: flags & 64 != 0,
                phase_program_revision: match self.sync_phase_revision.load(Ordering::SeqCst) {
                    0 => None,
                    revision => Some(revision),
                },
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
