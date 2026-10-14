//! Ordinary preparation separates complete disk integrity from selected live PCM.
#![cfg(windows)]

use super::*;
use crate::audio_engine::cold_jobs::PCM_LIMIT_BYTES;
use crate::audio_engine::cold_residency::ResidentLoadHint;
use crate::audio_engine::cold_store::{
    WindowReadObservation, reset_window_read_observation_for_test, window_read_observation_for_test,
};
use crate::audio_engine::initial_loop_start::detect_initial_loop_start;
use crate::audio_engine::material_migration::{prepare_disk_pcm, prepare_pcm};

struct Fixture {
    _temp: tempfile::TempDir,
    samples: PathBuf,
    source: PathBuf,
    audio: Vec<f32>,
    rate: u32,
    channels: usize,
}

impl Fixture {
    fn new(audio: Vec<f32>, rate: u32, channels: usize) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("incoming.wav");
        let length = u32::try_from(audio.len() * 4).unwrap();
        let mut writer = File::create(&source).unwrap();
        writer.write_all(b"RIFF").unwrap();
        writer.write_all(&(length + 36).to_le_bytes()).unwrap();
        writer.write_all(b"WAVEfmt \x10\0\0\0\x03\0").unwrap();
        writer.write_all(&(channels as u16).to_le_bytes()).unwrap();
        writer.write_all(&rate.to_le_bytes()).unwrap();
        writer
            .write_all(&(rate * channels as u32 * 4).to_le_bytes())
            .unwrap();
        writer
            .write_all(&(channels as u16 * 4).to_le_bytes())
            .unwrap();
        writer.write_all(&32_u16.to_le_bytes()).unwrap();
        writer.write_all(b"data").unwrap();
        writer.write_all(&length.to_le_bytes()).unwrap();
        for value in &audio {
            writer.write_all(&value.to_le_bytes()).unwrap();
        }
        drop(writer);
        Self {
            samples: temp.path().join("samples"),
            source,
            audio,
            rate,
            channels,
            _temp: temp,
        }
    }

    fn capture(&self) -> ColdTransaction {
        ColdTransaction::capture(&self.samples, &self.source, true, &|| false).unwrap()
    }

    fn prepare(&self) -> ColdTransaction {
        let mut transaction = self.capture();
        let _gate = transaction
            .preparation_gate(self.rate, self.channels, &|| false)
            .unwrap();
        prepare_disk_pcm(
            &mut transaction,
            &self.source,
            self.rate,
            self.channels,
            PCM_LIMIT_BYTES,
            &|| false,
            &mut |_| {},
        )
        .unwrap();
        transaction
    }

    fn hint(&self, start: usize, end: usize) -> Option<ResidentLoadHint> {
        Some(ResidentLoadHint {
            start_s: start as f64 / f64::from(self.rate),
            end_s: end as f64 / f64::from(self.rate),
            key_lock: false,
        })
    }

    fn commit(
        &self,
        mut transaction: ColdTransaction,
        sample: &SampleBuffer,
    ) -> CommittedColdLease {
        transaction.commit(&|| false).unwrap();
        transaction.bind_sample(sample).unwrap();
        transaction.into_lease()
    }
}

#[test]
fn prepared_equal_finite_selections_share_pcm_with_fresh_views_and_complete_verification() {
    let fixture = Fixture::new(vec![0.125; 1024], 8_000, 2);
    let mut first = fixture.prepare();
    let selected = first
        .read_prepared_selection(fixture.hint(17, 41), PCM_LIMIT_BYTES, &|| false)
        .unwrap();
    let lease = fixture.commit(first, &selected);
    let mut warm = fixture.capture();
    let _gate = warm
        .preparation_gate(fixture.rate, fixture.channels, &|| false)
        .unwrap();
    assert!(
        warm.try_reuse_prepared(fixture.rate, fixture.channels, PCM_LIMIT_BYTES, &|| false)
            .unwrap()
    );
    reset_window_read_observation_for_test();
    let restored = warm
        .read_prepared_selection(fixture.hint(17, 41), PCM_LIMIT_BYTES, &|| false)
        .unwrap();
    assert!(Arc::ptr_eq(&selected.samples, &restored.samples));
    let original_view = selected.residency.as_ref().unwrap();
    let restored_view = restored.residency.as_ref().unwrap();
    assert!(Arc::ptr_eq(&original_view.source, &restored_view.source));
    assert!(!Arc::ptr_eq(original_view, restored_view));
    assert_eq!(
        window_read_observation_for_test(),
        WindowReadObservation::default()
    );
    assert_eq!(
        warm.integrity.decoder_verify_bytes,
        fixture.audio.len() as u64 * 4
    );
    assert_eq!(
        warm.integrity.playback_verify_bytes,
        fixture.audio.len() as u64 * 4
    );
    assert!(lease.live_cached_pcm_samples().is_none());
    let different = warm
        .read_prepared_selection(fixture.hint(18, 41), PCM_LIMIT_BYTES, &|| false)
        .unwrap();
    assert!(!Arc::ptr_eq(&selected.samples, &different.samples));
    assert_eq!(window_read_observation_for_test().read_bytes, 23 * 2 * 4);
}

#[test]
fn cold_disk_preparation_and_chunk_activity_allocate_no_selected_pcm() {
    let channels = 2;
    let chunk_frames = CHUNK_BYTES / channels / 4;
    let frames = chunk_frames * 2 + 7;
    for activity_frame in [
        0,
        chunk_frames - 1,
        chunk_frames,
        chunk_frames + 2,
        frames - 1,
    ] {
        let mut audio = vec![0.0; frames * channels];
        for frame in audio.chunks_exact_mut(channels) {
            frame.copy_from_slice(&[0.01, -0.01]);
        }
        audio[activity_frame * channels..activity_frame * channels + channels]
            .copy_from_slice(&[0.0101, -0.0101]);
        let fixture = Fixture::new(audio, 44_100, channels);
        reset_window_read_observation_for_test();
        let mut transaction = fixture.prepare();
        assert_eq!(transaction.integrity.playback_read_bytes, 0);
        assert_eq!(
            window_read_observation_for_test(),
            WindowReadObservation::default()
        );
        let expected = detect_initial_loop_start(
            &SampleBuffer {
                residency: None,
                samples: Arc::from(fixture.audio.as_slice()),
                channels,
            },
            fixture.rate,
        );
        assert_eq!(
            transaction
                .detect_prepared_initial_loop_start(&|| false)
                .unwrap(),
            expected
        );
        assert_eq!(
            window_read_observation_for_test(),
            WindowReadObservation::default()
        );
        assert!(transaction.integrity.playback_read_bytes > 0);
        let start = activity_frame.saturating_sub(1);
        let end = (start + 5).min(frames);
        let selected = transaction
            .read_prepared_selection(
                fixture.hint(start, end),
                CHUNK_BYTES + (end - start) * channels * 4,
                &|| false,
            )
            .unwrap();
        assert_eq!(
            (selected.resident_start(), selected.resident_end()),
            (start, end)
        );
        assert_eq!(selected.frame_count(), frames);
        assert_eq!(
            selected.samples.as_ref(),
            &fixture.audio[start * channels..end * channels]
        );
        assert_eq!(
            window_read_observation_for_test().allocated_bytes,
            (end - start) * channels * 4
        );
    }
}

#[test]
fn quiet_complete_playback_has_no_activity_candidate_and_no_pcm_allocation() {
    let fixture = Fixture::new(vec![0.0, 0.005, -0.005, 0.01, -0.01], 96_000, 1);
    let mut transaction = fixture.prepare();
    reset_window_read_observation_for_test();
    assert_eq!(
        transaction
            .detect_prepared_initial_loop_start(&|| false)
            .unwrap(),
        None
    );
    assert_eq!(
        transaction.integrity.playback_read_bytes,
        fixture.audio.len() as u64 * 4
    );
    assert_eq!(
        window_read_observation_for_test(),
        WindowReadObservation::default()
    );
}

#[test]
fn composed_prepare_pcm_reuses_live_complete_warm_backing_without_another_read() {
    let fixture = Fixture::new(vec![0.0, 0.005, 0.5, -0.5], 48_000, 2);
    let mut first = fixture.prepare();
    let complete = first
        .read_prepared_selection(None, PCM_LIMIT_BYTES, &|| false)
        .unwrap();
    let lease = fixture.commit(first, &complete);
    let mut warm = fixture.capture();
    let _gate = warm
        .preparation_gate(fixture.rate, fixture.channels, &|| false)
        .unwrap();
    reset_window_read_observation_for_test();
    let selected = prepare_pcm(
        &mut warm,
        &fixture.source,
        fixture.rate,
        fixture.channels,
        PCM_LIMIT_BYTES,
        None,
        &|| false,
        &mut |_| {},
    )
    .unwrap();
    assert!(Arc::ptr_eq(&complete.samples, &selected.samples));
    assert!(warm.integrity.warm);
    assert_eq!(
        warm.integrity.decoder_verify_bytes,
        fixture.audio.len() as u64 * 4
    );
    assert_eq!(
        warm.integrity.playback_verify_bytes,
        fixture.audio.len() as u64 * 4
    );
    assert_eq!(warm.integrity.playback_read_bytes, 0);
    assert_eq!(
        window_read_observation_for_test(),
        WindowReadObservation::default()
    );
    assert_eq!(warm.manifest().unwrap().identity, lease.manifest.identity);
}

#[test]
fn prepared_warm_integrity_succeeds_below_full_extent_then_only_selected_pcm_is_read() {
    let fixture = Fixture::new(vec![0.005; CHUNK_BYTES], 48_000, 2);
    let mut first = fixture.prepare();
    let seed = first
        .read_prepared_selection(fixture.hint(0, 1), PCM_LIMIT_BYTES, &|| false)
        .unwrap();
    let lease = fixture.commit(first, &seed);
    let mut warm = fixture.capture();
    let _gate = warm
        .preparation_gate(fixture.rate, fixture.channels, &|| false)
        .unwrap();
    reset_window_read_observation_for_test();
    let maximum = CHUNK_BYTES + 1024;
    assert!(fixture.audio.len() * 4 > maximum);
    assert!(
        warm.try_reuse_prepared(fixture.rate, fixture.channels, maximum, &|| false)
            .unwrap()
    );
    assert_eq!(warm.integrity.playback_read_bytes, 0);
    assert_eq!(
        warm.integrity.playback_verify_bytes,
        fixture.audio.len() as u64 * 4
    );
    assert_eq!(
        warm.integrity.decoder_verify_bytes,
        fixture.audio.len() as u64 * 4
    );
    assert_eq!(
        window_read_observation_for_test(),
        WindowReadObservation::default()
    );
    assert!(
        warm.read_prepared_selection(None, maximum, &|| false)
            .is_err()
    );
    let selected = warm
        .read_prepared_selection(fixture.hint(17, 41), maximum, &|| false)
        .unwrap();
    assert_eq!(
        (selected.resident_start(), selected.resident_end()),
        (17, 41)
    );
    assert_eq!(window_read_observation_for_test().read_bytes, 24 * 2 * 4);
    assert!(lease.live_cached_pcm_samples().is_none());
}

#[test]
fn disk_only_warm_rejects_late_corruption_and_regenerates_without_selecting_pcm() {
    for name in ["decoder.f32le", "playback.f32le"] {
        for mode in ["late-finite", "late-nonfinite", "truncate", "extra-eof"] {
            let fixture = Fixture::new(vec![0.005; CHUNK_BYTES], 48_000, 2);
            let mut first = fixture.prepare();
            let seed = first
                .read_prepared_selection(fixture.hint(0, 1), PCM_LIMIT_BYTES, &|| false)
                .unwrap();
            let lease = fixture.commit(first, &seed);
            let path = lease.cache_path.join(name);
            drop((lease, seed));
            let mut bytes = fs::read(&path).unwrap();
            let length = bytes.len();
            match mode {
                "late-finite" => bytes[length - 4] ^= 1,
                "late-nonfinite" => bytes[length - 4..].copy_from_slice(&f32::NAN.to_le_bytes()),
                "truncate" => bytes.truncate(length - 4),
                "extra-eof" => bytes.extend_from_slice(&0_f32.to_le_bytes()),
                _ => unreachable!(),
            }
            fs::write(&path, &bytes).unwrap();
            reset_window_read_observation_for_test();
            let regenerated = fixture.prepare();
            assert!(!regenerated.integrity.warm, "accepted {mode} in {name}");
            assert_eq!(regenerated.integrity.playback_read_bytes, 0);
            assert_eq!(
                window_read_observation_for_test(),
                WindowReadObservation::default()
            );
            assert_eq!(fs::read(&path).unwrap(), bytes);
            assert!(fixture.source.exists());
        }
    }
}

#[test]
fn activity_scan_cancellation_after_a_real_chunk_returns_no_pcm_and_owned_rollback() {
    use std::cell::Cell;
    let fixture = Fixture::new(vec![0.005; CHUNK_BYTES], 48_000, 2);
    let mut transaction = fixture.prepare();
    let staging = transaction.staging.clone().unwrap();
    let calls = Cell::new(0);
    reset_window_read_observation_for_test();
    let error = transaction
        .detect_prepared_initial_loop_start(&|| {
            calls.set(calls.get() + 1);
            calls.get() >= 3
        })
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Interrupted);
    assert_eq!(
        transaction.integrity.playback_read_bytes,
        CHUNK_BYTES as u64
    );
    assert_eq!(
        window_read_observation_for_test(),
        WindowReadObservation::default()
    );
    drop(transaction);
    assert!(!staging.exists());
    assert!(fixture.source.exists());
}

#[test]
fn activity_scan_refuses_another_file_identity_even_with_the_same_pcm_extent() {
    let fixture = Fixture::new(vec![0.0, 0.005, 0.5, -0.5], 48_000, 2);
    let mut transaction = fixture.prepare();
    let staging = transaction.staging.clone().unwrap();
    let other = fixture._temp.path().join("unowned");
    fs::create_dir(&other).unwrap();
    fs::write(
        other.join("playback.f32le"),
        fs::read(staging.join("playback.f32le")).unwrap(),
    )
    .unwrap();
    transaction.staging = Some(other.clone());
    reset_window_read_observation_for_test();
    let error = transaction
        .detect_prepared_initial_loop_start(&|| false)
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert_eq!(transaction.integrity.playback_read_bytes, 0);
    assert_eq!(
        window_read_observation_for_test(),
        WindowReadObservation::default()
    );
    transaction.staging = Some(staging);
    drop(transaction);
    assert!(other.join("playback.f32le").exists());
}

#[test]
fn temporary_complete_analysis_backing_is_released_before_a_finite_selection() {
    let fixture = Fixture::new(vec![0.005; CHUNK_BYTES], 48_000, 2);
    let mut first = fixture.prepare();
    let seed = first
        .read_prepared_selection(fixture.hint(0, 1), PCM_LIMIT_BYTES, &|| false)
        .unwrap();
    let lease = fixture.commit(first, &seed);
    let mut warm = fixture.capture();
    let _gate = warm
        .preparation_gate(fixture.rate, fixture.channels, &|| false)
        .unwrap();
    assert!(
        warm.try_reuse_prepared(fixture.rate, fixture.channels, PCM_LIMIT_BYTES, &|| false)
            .unwrap()
    );
    let complete = warm
        .read_prepared_selection(None, PCM_LIMIT_BYTES, &|| false)
        .unwrap();
    assert!(
        lease
            .cache
            .pcm
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .upgrade()
            .is_some()
    );
    drop(complete);
    assert_eq!(
        lease
            .cache
            .pcm
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .strong_count(),
        0
    );
    let selected = warm
        .read_prepared_selection(fixture.hint(17, 41), CHUNK_BYTES + 1024, &|| false)
        .unwrap();
    assert_eq!(selected.samples.len(), 24 * 2);
    assert!(lease.cache.pcm.lock().unwrap().is_none());
}
