use super::*;
use crate::audio_engine::input_mapping::launch_admission_tests::LaunchFixture;
use crate::audio_engine::prepared_source::content_tests::{prepared_set, ticket};

fn observation(
    fixture: &LaunchFixture,
    ticket: &PreparedSourceTicket,
    mask: u8,
) -> LegacyStemObservation {
    LegacyStemObservation {
        id: ticket.id,
        epoch: 1,
        ownership: fixture.engine.input_runtime_ownership.clone(),
        publication: ticket.publication.clone(),
        binding: ticket.admission.binding,
        generation: ticket.admission.source_generation,
        rate: ticket.sample_rate_hz,
        source_hash: crate::audio_engine::stem_cache::source_version_hash(&ticket.source_version),
        mode: StemMixMode::FullMix,
        mask,
        status: AtomicU8::new(0),
        effect_revision: AtomicU64::new(0),
    }
}

#[test]
fn original_legacy_mode_and_mask_require_callback_effect_and_invalidate_on_full_mix_mask_change() {
    let fixture = LaunchFixture::new(8);
    let ticket = ticket(&fixture);
    let mut mixer = fixture.mixer();
    mixer.set_prepared_source_epochs(fixture.engine.prepared_source_epochs.clone());
    assert!(mixer.publish_prepared_stems(0, prepared_set(&ticket)));
    fixture
        .engine
        .input_runtime_ownership
        .output_change_epoch
        .store(1, Ordering::Release);
    let observation = observation(&fixture, &ticket, 5);
    assert!(!observation.current());
    mixer.observe_legacy_stem_restore(&observation);
    assert_eq!(observation.status.load(Ordering::Acquire), 1);
    assert!(observation.current());
    assert!(mixer.set_stem_enabled_mask(0, 9, observation.source_hash));
    assert!(!observation.current());
    observation.reject();
    assert_eq!(
        observation.status.load(Ordering::Acquire),
        1,
        "accepted history is immutable"
    );
}

#[test]
fn equal_pcm_and_source_context_cannot_replace_the_original_stem_publication() {
    let fixture = LaunchFixture::new(8);
    let original = ticket(&fixture);
    let mut mixer = fixture.mixer();
    mixer.set_prepared_source_epochs(fixture.engine.prepared_source_epochs.clone());
    assert!(mixer.publish_prepared_stems(0, prepared_set(&original)));
    let unrelated = ticket(&fixture);
    assert!(original.same_source_request(&unrelated));
    assert!(
        !original
            .publication
            .same_publication(&unrelated.publication)
    );
    assert!(mixer.publish_prepared_stems(0, prepared_set(&unrelated)));
    fixture
        .engine
        .input_runtime_ownership
        .output_change_epoch
        .store(1, Ordering::Release);
    let observation = observation(&fixture, &original, 5);
    mixer.observe_legacy_stem_restore(&observation);
    assert_eq!(observation.status.load(Ordering::Acquire), 2);
    assert!(!observation.current());
}
