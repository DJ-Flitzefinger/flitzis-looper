//! Fixed observation of the last actual master publication, never source authority.

use super::input_runtime_binding::InputRuntimePadBinding;
use sha2::{Digest, Sha256};
use std::fmt::Write;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum MasterProvenance {
    #[default]
    Absent,
    Unknown,
    Qualified {
        source_identity: [u8; 32],
        accepted_revision: Option<[u8; 32]>,
    },
}

impl MasterProvenance {
    pub(crate) fn qualified(
        source_identity: [u8; 32],
        binding: super::input_runtime_binding::InputPadBinding,
    ) -> Self {
        Self::Qualified {
            source_identity,
            accepted_revision: binding.accepted.map(|accepted| accepted.revision),
        }
    }

    pub(crate) fn kind(self) -> &'static str {
        match self {
            Self::Absent => "absent",
            Self::Unknown => "unknown",
            Self::Qualified { .. } => "qualified",
        }
    }

    pub(crate) fn source_identity(self) -> Option<[u8; 32]> {
        match self {
            Self::Qualified {
                source_identity, ..
            } => Some(source_identity),
            _ => None,
        }
    }

    pub(crate) fn accepted_revision(self) -> Option<[u8; 32]> {
        match self {
            Self::Qualified {
                accepted_revision, ..
            } => accepted_revision,
            _ => None,
        }
    }
}

/// Control-only correlation of the complete original source. Placement, timing
/// authority and resident windows may change without changing that source.
pub(crate) fn source_identity(binding: &InputRuntimePadBinding) -> Option<[u8; 32]> {
    let snapshot = binding.binding.placement?;
    let key = snapshot.content_key()?;
    let mut digest = Sha256::new();
    digest.update(b"looper-master-original-source-v1\0");
    for value in [
        snapshot.owner().diagnostic() as u64,
        key.cell().index() as u64,
        key.lifetime(),
        binding.source_generation,
        binding.binding.source_address as u64,
        binding.binding.sample_count as u64,
        binding.binding.channels as u64,
        u64::from(binding.binding.sample_rate_hz),
    ] {
        digest.update(value.to_le_bytes());
    }
    Some(digest.finalize().into())
}

/// Presentation only; called with the GIL on the control side, never in render.
pub(crate) fn hex(value: [u8; 32]) -> String {
    let mut result = String::with_capacity(64);
    for byte in value {
        write!(&mut result, "{byte:02x}").expect("writing into a String");
    }
    result
}

pub(crate) fn accepted_revision(value: [u8; 32]) -> String {
    format!("accepted-constant-timing-v1:{}", hex(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_engine::input_runtime_binding::InputRuntimeOwnership;
    use crate::messages::{ResidentBinding, ResidentContext};
    use std::sync::Arc;

    #[test]
    fn original_source_correlation_excludes_window_placement_and_authority_rebasing() {
        let owner = Arc::new(InputRuntimeOwnership::tracked());
        let original = InputRuntimePadBinding::for_test(0, owner);
        let identity = source_identity(&original).unwrap();
        let mut rebased = original.clone();
        rebased.id = 1;
        rebased.binding.authority_revision += 1;
        rebased.binding.runtime_revision += 1;
        rebased.binding.resident = Some(ResidentBinding {
            address: 123,
            start: 12,
            end: 24,
            revision: 7,
            context: ResidentContext::KeyLockFiniteLoop,
        });
        assert_eq!(source_identity(&rebased), Some(identity));
        let changes: [fn(&mut InputRuntimePadBinding); 5] = [
            |binding| binding.source_generation += 1,
            |binding| binding.binding.source_address += 1,
            |binding| binding.binding.sample_count += 1,
            |binding| binding.binding.channels += 1,
            |binding| binding.binding.sample_rate_hz += 1,
        ];
        for change in changes {
            let mut replaced = original.clone();
            change(&mut replaced);
            assert_ne!(source_identity(&replaced), Some(identity));
        }
        let mut reincarnated = original.clone();
        let placement = original.binding.placement.unwrap();
        reincarnated.binding.placement = placement.for_lifetime(placement.lifetime() + 1);
        assert_ne!(source_identity(&reincarnated), Some(identity));
        let mut foreign =
            InputRuntimePadBinding::for_test(0, Arc::new(InputRuntimeOwnership::tracked()));
        foreign.binding.placement = foreign
            .binding
            .placement
            .unwrap()
            .for_lifetime(placement.lifetime());
        assert_ne!(source_identity(&foreign), Some(identity));
    }
}
