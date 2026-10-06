//! Immutable source bindings and lossless backend adapters for offline assessment.
//!
//! The original-file digest and transformation provenance are caller evidence.
//! Actual borrowed PCM/input digests, dimensions and request associations are
//! checked here. No adapter verifies musical units or publishes runtime timing.

mod backend;
mod binding;

pub use backend::{
    BackendEvidence, BeatThisModelIdentity, BeatThisRawEvidence, BeatThisRequestIdentity,
    BoundTempoEvidence, QmInputDescriptor, QmInputTransform, TimingBound,
};
pub use binding::{
    JobIdentity, MONO_REVISION, PcmBinding, PcmBindingMetadata, TempoEvidenceError, f32_pcm_sha256,
    f64_input_sha256,
};
