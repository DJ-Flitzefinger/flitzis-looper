//! Rebind acknowledged immutable Automatic evidence to an unpublished copy.
//! Custody checks and record allocation are worker-only; no PCM is read or converted.

use super::*;
use crate::audio_engine::cold_store::CommittedColdLease;
use crate::audio_engine::material_paths::{self, AssetKind};
use crate::audio_engine::project_assets::{capture_identity, reject_links};

/// Prepare a future target record, leaving source and target publication state unchanged.
///
/// The caller captures the acknowledged record and retains independent verified
/// target custody before this detached work. Source lifetime/current intent is
/// rechecked by the Copy transaction at claim, not by historical evidence here.
/// The supplied permit belongs to the target's actual, still-unadvanced epoch.
// Future request, source generation and publication epoch have separate authority.
#[allow(clippy::too_many_arguments)]
pub(in crate::audio_engine) fn rebind_copy_timing(
    captured: &CurrentConstantTimingRecord,
    target: &SampleBuffer,
    target_lease: &CommittedColdLease,
    target_id: usize,
    request_id: u64,
    source_generation: u64,
    future_epoch: u64,
    publication: PreparedSourcePermit,
) -> Result<(CurrentConstantTimingRecord, PreparedConstantTiming), String> {
    if target_id >= NUM_SAMPLES || request_id == 0 || source_generation == 0 || future_epoch == 0 {
        return Err("copy timing future identity is invalid".into());
    }
    if publication.expected != future_epoch
        || publication.epoch.load(Ordering::Acquire) >= future_epoch
        || !matches!(publication.status(), "captured" | "pending")
        || Arc::ptr_eq(&publication.epoch, &captured.publication.epoch)
    {
        return Err("copy timing requires an unpublished independent target permit".into());
    }
    if captured.publication.status() != "accepted"
        || captured.publication_epoch == 0
        || captured.publication.expected != captured.publication_epoch
        || captured.projection
            != projection(
                &captured.accepted,
                captured.binding.sample_rate_hz,
                captured.publication_epoch,
            )?
        || captured.revision != captured.accepted.revision()
        || captured.raw_revision != captured.accepted.evidence().source_identity().raw_revision
        || captured.period_seconds.to_bits()
            != captured.accepted.period_seconds_per_quarter().to_bits()
        || captured.origin.seconds.to_bits() != captured.accepted.origin().seconds.to_bits()
        || captured.origin.provenance != captured.accepted.origin().provenance
        || captured.decision.policy_version != captured.accepted.decision().policy_version
        || captured.decision.provenance != captured.accepted.decision().provenance
    {
        return Err("copy timing capture is not coherent acknowledged evidence".into());
    }
    let view = target
        .residency
        .as_ref()
        .ok_or("copy timing target has no complete source descriptor")?;
    let source = &view.source;
    let sample_count = source
        .frame_count
        .checked_mul(source.channels)
        .ok_or("copy timing complete source extent overflow")?;
    let digest = |bytes: &[u8; 32]| {
        bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    if !target.valid_residency(source.sample_rate_hz, source.channels)
        || captured.source_address != target.source_address()
        || captured.sample_count != sample_count
        || captured.channels != source.channels
        || captured.binding.source_sha256 != digest(&source.original_sha256)
        || captured.binding.pcm_sha256 != digest(&source.mono_sha256)
        || captured.binding.sample_rate_hz != source.sample_rate_hz
        || captured.binding.frame_count
            != u64::try_from(source.frame_count).map_err(|_| "copy timing frame extent overflow")?
        || source.source_zero_frame != 0
        || captured.binding.origin_seconds.to_bits() != 0.0_f64.to_bits()
        || captured.binding.mono_revision != MONO_REVISION
        || captured.binding.source_provenance != SOURCE_PROVENANCE
        || target_lease.manifest.descriptor["playback"]["pcm"]["mono_revision"] != MONO_REVISION
        || target_lease.manifest.descriptor["playback"]["pcm"]["source_zero_bits"]
            != "0000000000000000"
    {
        return Err("copy timing capture differs from the complete target source".into());
    }
    if target_lease.assignment_retired() {
        return Err("copy timing target custody is retired".into());
    }
    let original =
        material_paths::resolve(target_lease.samples_root(), &target_lease.original_path)
            .map_err(|error| error.to_string())?;
    if original.kind
        != (AssetKind::Original {
            material: target_lease.material_id.clone(),
        })
    {
        return Err("copy timing original custody has a different typed binding".into());
    }
    reject_links(&target_lease.original_path).map_err(|error| error.to_string())?;
    let original_identity = capture_identity(&target_lease.original_path)
        .map_err(|error| error.to_string())?
        .ok_or("copy timing original custody is missing")?;
    if !target_lease.original_identity_matches(&original_identity) {
        return Err("copy timing original custody FileID changed".into());
    }
    // Validate the held complete PCM FileID/extent without reading any samples.
    let _reader = target_lease
        .open_complete_reader(target)
        .map_err(|error| error.to_string())?;
    let mut binding = captured.binding.clone();
    binding.job = JobIdentity {
        pad_id: target_id as u64,
        request_id,
        source_id: format!("loaded-{target_id}-{source_generation}"),
        source_generation,
    };
    captured
        .accepted
        .check_source_binding(&binding)
        .map_err(|error| error.to_string())?;
    let projection = projection(&captured.accepted, source.sample_rate_hz, future_epoch)?;
    let record = CurrentConstantTimingRecord {
        content_key: None,
        source_address: target.source_address(),
        sample_count,
        channels: target.channels,
        binding,
        publication_epoch: future_epoch,
        publication: publication.clone(),
        projection,
        ..captured.clone()
    };
    Ok((
        record,
        PreparedConstantTiming {
            content_key: None,
            reference: target.clone(),
            publication,
            projection,
        },
    ))
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::audio_engine::cold_store::{
        reset_window_read_observation_for_test, window_read_observation_for_test,
    };
    use crate::audio_engine::material_migration::prepare_material_complete_for_test;
    use crate::messages::ResidentContext;
    use std::fs;

    struct Fixture {
        engine: AudioEngine,
        sample: SampleBuffer,
        lease: CommittedColdLease,
        captured: CurrentConstantTimingRecord,
        _directory: tempfile::TempDir,
    }

    impl Fixture {
        fn new() -> Self {
            Python::initialize();
            let directory = tempfile::tempdir().unwrap();
            let root = directory.path().join("samples");
            fs::create_dir(&root).unwrap();
            let input = root.join("copy-timing.wav");
            let declared = super::super::tests::source();
            let length = u32::try_from(declared.samples.len() * 4).unwrap();
            let mut encoded = Vec::with_capacity(length as usize + 44);
            encoded.extend_from_slice(b"RIFF");
            encoded.extend_from_slice(&(length + 36).to_le_bytes());
            encoded.extend_from_slice(b"WAVEfmt \x10\0\0\0\x03\0\x01\0");
            encoded.extend_from_slice(&super::super::tests::RATE.to_le_bytes());
            encoded.extend_from_slice(&(super::super::tests::RATE * 4).to_le_bytes());
            encoded.extend_from_slice(&4_u16.to_le_bytes());
            encoded.extend_from_slice(&32_u16.to_le_bytes());
            encoded.extend_from_slice(b"data");
            encoded.extend_from_slice(&length.to_le_bytes());
            for value in declared.samples.iter() {
                encoded.extend_from_slice(&value.to_le_bytes());
            }
            fs::write(&input, encoded).unwrap();
            let material = prepare_material_complete_for_test(
                &root,
                &input,
                super::super::tests::RATE,
                1,
                &|| false,
            )
            .unwrap();
            let sample = material.sample.clone();
            let lease = material.lease.clone();
            let digest = lease.manifest.descriptor["decoder"]["original"]["sha256"]
                .as_str()
                .unwrap()
                .to_owned();
            let engine = AudioEngine::new().unwrap();
            engine.sample_cache.lock().unwrap()[0] = Some(sample.clone());
            engine.pad_request_ids.lock().unwrap()[0] = 7;
            engine.loaded_source_generations.lock().unwrap()[0] = (7, super::super::tests::RATE);
            engine.loaded_source_digests.lock().unwrap()[0] = Some(digest);
            engine.cold_leases.lock().unwrap()[0] = Some(lease.clone());
            engine.cold_lease_generations[0].store(7, Ordering::Release);
            engine.timing_intents.lock().unwrap()[0] = TimingIntent::Automatic;
            engine.input_runtime_ownership.publish_test_source(
                0,
                &sample,
                super::super::tests::RATE,
                7,
            );
            engine
                .input_runtime_ownership
                .set_timing_intent(0, TimingIntent::Automatic);
            let (producer, mut consumer) = super::super::tests::queue(1);
            let engine = super::super::tests::with_control_producer(engine, &producer);
            super::super::l3_constant_timing_fixture::publish_synthetic_loaded_timing(
                &engine,
                &producer,
                0,
                super::super::tests::PERIOD,
            );
            let mut mixer = super::super::tests::acknowledged_mixer(&engine);
            assert!(super::super::tests::accept_message(
                &mut mixer,
                consumer.pop().unwrap(),
            ));
            let captured = engine.current_constant_timing.lock().unwrap()[0]
                .last()
                .unwrap()
                .clone();
            assert_eq!(captured.publication.status(), "accepted");
            Self {
                _directory: directory,
                engine,
                sample,
                lease,
                captured,
            }
        }

        fn target(&self, finite: bool) -> (SampleBuffer, CommittedColdLease) {
            let lease = self.lease.subscribe_verified().unwrap();
            let (start, end, context) = if finite {
                (100, 300, ResidentContext::FiniteLoop)
            } else {
                (0, self.sample.frame_count(), ResidentContext::FullTrack)
            };
            let sample = lease
                .read_window_cancellable(
                    &self.sample,
                    start,
                    end,
                    11,
                    context,
                    MAX_PCM_BYTES,
                    &|| false,
                )
                .unwrap();
            (sample, lease)
        }
    }

    fn permit() -> PreparedSourcePermit {
        PreparedSourcePermit::new(Arc::new(AtomicU64::new(0)), 1)
    }

    #[test]
    fn full_and_finite_copy_rebind_without_pcm_work_or_reusing_ack() {
        let fixture = Fixture::new();
        for (id, finite) in [(36, false), (215, true)] {
            let (sample, lease) = fixture.target(finite);
            let lifetime = crate::audio_engine::content_lifetime::reserve().unwrap();
            let target = fixture
                .engine
                .input_runtime_ownership
                .slot_snapshot(id)
                .unwrap()
                .for_lifetime(lifetime)
                .unwrap();
            let target_key = target.content_key().unwrap();
            let base_epoch = fixture.engine.prepared_source_epochs[id].load(Ordering::Acquire);
            let future_epoch = base_epoch + 1;
            let publication = PreparedSourcePermit::new(
                fixture.engine.prepared_source_epochs[id].clone(),
                future_epoch,
            );
            let source_epoch = fixture.captured.publication.epoch.load(Ordering::Acquire);
            reset_window_read_observation_for_test();
            let (mut record, mut prepared) = rebind_copy_timing(
                &fixture.captured,
                &sample,
                &lease,
                id,
                23,
                19,
                future_epoch,
                publication.clone(),
            )
            .unwrap();
            assert!(prepared.content_key.is_none());
            prepared.content_key = Some(target_key);
            record.content_key = Some(target_key);
            assert!(
                !fixture
                    .engine
                    .input_runtime_ownership
                    .content_key_current(target_key)
            );
            let observed = window_read_observation_for_test();
            assert_eq!(observed.read_bytes, 0);
            assert_eq!(observed.allocated_bytes, 0);
            assert_eq!(record.binding.job.pad_id, id as u64);
            assert_eq!(record.binding.job.request_id, 23);
            assert_eq!(record.binding.job.source_generation, 19);
            assert_eq!(record.binding.job.source_id, format!("loaded-{id}-19"));
            assert_eq!(
                record.binding.frame_count,
                fixture.sample.frame_count() as u64,
            );
            assert_eq!(record.revision, fixture.captured.revision);
            assert_eq!(record.raw_revision, fixture.captured.raw_revision);
            assert!(Arc::ptr_eq(&record.accepted, &fixture.captured.accepted));
            assert!(Arc::ptr_eq(&prepared.reference.samples, &sample.samples));
            assert_eq!(publication.epoch.load(Ordering::Acquire), base_epoch);
            assert_eq!(publication.status(), "captured");
            assert!(!publication.current());
            assert_eq!(
                fixture.captured.publication.epoch.load(Ordering::Acquire),
                source_epoch,
            );
            assert_eq!(
                fixture
                    .engine
                    .current_timing_acknowledgements
                    .current_for_slot(&fixture.engine.input_runtime_ownership, id),
                0,
            );
            assert!(!Arc::ptr_eq(
                &record.publication.epoch,
                &fixture.captured.publication.epoch,
            ));
            let records = [record];
            let digest = records[0].binding.source_sha256.as_str();
            assert!(
                current_record_for_source(
                    &fixture.engine,
                    id,
                    &sample,
                    (19, source_rate(&sample)),
                    Some(digest),
                    &records,
                )
                .is_none()
            );
            // Exercise the genuine timing handler/ACK. This does not assert a Copy ACK.
            // Its target source must first acquire the reserved Native incarnation, exactly
            // as the complete Copy publisher does before acknowledging target timing.
            fixture
                .engine
                .input_runtime_ownership
                .publish_content_lifetime(id, lifetime);
            fixture.engine.input_runtime_ownership.publish_source(
                id,
                &sample,
                source_rate(&sample),
                19,
            );
            assert!(
                fixture
                    .engine
                    .input_runtime_ownership
                    .content_key_current(target_key)
            );
            publication.mark_pending().unwrap();
            publication.epoch.store(future_epoch, Ordering::Release);
            let mut mixer = crate::audio_engine::audio_stream::create_stream_mixer(
                1,
                super::super::tests::RATE,
                fixture.engine.current_timing_acknowledgements.clone(),
                fixture.engine.input_runtime_ownership.clone(),
                fixture.engine.prepared_source_epochs.clone(),
            )
            .unwrap();
            mixer.load_sample(id, sample.clone());
            assert!(super::super::tests::accept_message(
                &mut mixer,
                ControlMessage::PublishConstantTiming {
                    id,
                    timing: prepared,
                },
            ));
            assert_eq!(publication.status(), "accepted");
            assert!(
                current_record_for_source(
                    &fixture.engine,
                    id,
                    &sample,
                    (19, source_rate(&sample)),
                    Some(digest),
                    &records,
                )
                .is_some()
            );
        }
    }

    fn source_rate(sample: &SampleBuffer) -> u32 {
        sample.residency.as_ref().unwrap().source.sample_rate_hz
    }

    #[test]
    fn old_accepted_or_source_epoch_permit_cannot_supply_copy_authority() {
        let fixture = Fixture::new();
        let (sample, lease) = fixture.target(false);
        let already_accepted = permit();
        already_accepted.mark_accepted();
        let wrong_expected = PreparedSourcePermit::new(Arc::new(AtomicU64::new(0)), 2);
        let source_epoch = PreparedSourcePermit::new(
            fixture.captured.publication.epoch.clone(),
            fixture.captured.publication_epoch + 1,
        );
        let source_future_epoch = source_epoch.expected;
        for (publication, future_epoch) in [
            (already_accepted, 1),
            (wrong_expected, 1),
            (source_epoch, source_future_epoch),
        ] {
            assert!(
                rebind_copy_timing(
                    &fixture.captured,
                    &sample,
                    &lease,
                    36,
                    23,
                    19,
                    future_epoch,
                    publication.clone(),
                )
                .is_err()
            );
        }
    }

    #[test]
    fn changed_content_dimensions_origin_rule_or_provenance_reject_without_publication() {
        let fixture = Fixture::new();
        let (sample, lease) = fixture.target(false);
        let mutations: [fn(&mut CurrentConstantTimingRecord); 8] = [
            |record| record.binding.source_sha256 = "c".repeat(64),
            |record| record.binding.pcm_sha256 = "d".repeat(64),
            |record| record.binding.sample_rate_hz *= 2,
            |record| record.binding.frame_count -= 1,
            |record| record.binding.origin_seconds = -0.0,
            |record| record.binding.mono_revision = "other-mono-rule".into(),
            |record| record.binding.source_provenance = "unsupported-source-proof".into(),
            |record| record.source_address += 1,
        ];
        for mutate in mutations {
            let mut captured = fixture.captured.clone();
            mutate(&mut captured);
            let publication = permit();
            assert!(
                rebind_copy_timing(
                    &captured,
                    &sample,
                    &lease,
                    36,
                    23,
                    19,
                    1,
                    publication.clone(),
                )
                .is_err()
            );
            assert_eq!(publication.epoch.load(Ordering::Acquire), 0);
            assert_eq!(publication.status(), "captured");
        }
    }

    #[test]
    fn independent_custody_survives_source_retirement_but_retired_target_rejects() {
        let fixture = Fixture::new();
        let (sample, lease) = fixture.target(true);
        fixture.lease.release_preparation_assignment();
        assert!(fixture.lease.assignment_retired());
        assert!(!lease.assignment_retired());
        assert!(
            rebind_copy_timing(&fixture.captured, &sample, &lease, 215, 23, 19, 1, permit(),)
                .is_ok()
        );
        lease.rollback_unadopted_cache();
        assert!(
            rebind_copy_timing(&fixture.captured, &sample, &lease, 215, 23, 19, 1, permit(),)
                .is_err()
        );
    }
}
