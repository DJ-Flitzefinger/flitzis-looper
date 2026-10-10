//! Real canonical PCM range I/O; no device, callback or persisted authority.
#![cfg(windows)]

use super::warm::{
    WindowReadObservation, reset_window_read_observation_for_test, window_read_observation_for_test,
};
use super::*;
use crate::audio_engine::cold_jobs::PCM_LIMIT_BYTES;
use crate::audio_engine::material_migration::prepare_material;
use crate::messages::{CompleteSourceIdentity, ResidentContext, ResidentSourceView};

const FRAMES: usize = 40_003;

struct Fixture {
    _temp: tempfile::TempDir,
    reference: SampleBuffer,
    lease: CommittedColdLease,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("samples");
        fs::create_dir(&root).unwrap();
        let source = root.join("range.wav");
        let length = u32::try_from(FRAMES * 2 * 2).unwrap();
        let mut writer = File::create(&source).unwrap();
        writer.write_all(b"RIFF").unwrap();
        writer.write_all(&(length + 36).to_le_bytes()).unwrap();
        writer.write_all(b"WAVEfmt \x10\0\0\0\x01\0\x02\0").unwrap();
        writer.write_all(&48_000_u32.to_le_bytes()).unwrap();
        writer.write_all(&192_000_u32.to_le_bytes()).unwrap();
        writer.write_all(&4_u16.to_le_bytes()).unwrap();
        writer.write_all(&16_u16.to_le_bytes()).unwrap();
        writer.write_all(b"data").unwrap();
        writer.write_all(&length.to_le_bytes()).unwrap();
        for frame in 0..FRAMES {
            for channel in 0..2 {
                let value = match (frame + channel) % 13 {
                    0 => i16::MIN,
                    1 => i16::MAX,
                    2 => 0,
                    _ => ((frame % 8192) as i16 - 4096) * (channel as i16 + 1),
                };
                writer.write_all(&value.to_le_bytes()).unwrap();
            }
        }
        drop(writer);
        let prepared = prepare_material(&root, &source, 48_000, 2, &|| false).unwrap();
        assert_eq!(prepared.sample.frame_count(), FRAMES);
        let reference = prepared
            .sample
            .window(11, 27, 2, ResidentContext::FiniteLoop)
            .unwrap();
        let lease = prepared.lease.clone();
        drop(prepared);
        assert!(
            lease.live_cached_pcm_samples().is_none(),
            "fixture kept complete PCM alive"
        );
        reset_window_read_observation_for_test();
        Self {
            _temp: temp,
            reference,
            lease,
        }
    }

    fn read(&self, start: usize, end: usize, maximum: usize) -> io::Result<SampleBuffer> {
        self.lease.read_window_cancellable(
            &self.reference,
            start,
            end,
            9,
            ResidentContext::FiniteLoop,
            maximum,
            &|| false,
        )
    }

    fn encoded(&self, start: usize, end: usize) -> Vec<u8> {
        let mut reader = File::open(self.lease.cache_path.join("playback.f32le")).unwrap();
        reader
            .seek(SeekFrom::Start((start * 2 * 4) as u64))
            .unwrap();
        let mut bytes = vec![0; (end - start) * 2 * 4];
        reader.read_exact(&mut bytes).unwrap();
        bytes
    }

    fn assert_bits(&self, sample: &SampleBuffer, start: usize, end: usize) {
        let expected = self.encoded(start, end);
        let actual = sample
            .samples
            .iter()
            .flat_map(|sample| sample.to_bits().to_le_bytes())
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
        assert!(sample.samples.iter().all(|sample| sample.is_finite()));
        assert_eq!(sample.frame_count(), FRAMES);
        assert_eq!(sample.resident_start(), start);
        assert_eq!(sample.resident_end(), end);
        assert_eq!(sample.window_revision(), 9);
        assert!(Arc::ptr_eq(
            &sample.residency.as_ref().unwrap().source,
            &self.reference.residency.as_ref().unwrap().source,
        ));
    }
}

#[test]
fn direct_distant_range_reads_exact_bytes_into_final_pcm_without_complete_expansion() {
    let fixture = Fixture::new();
    let (start, end) = (31_111, 31_148);
    let held = fixture.reference.samples.len() * 4;
    let allocated = (end - start) * 2 * 4;
    let maximum = held + allocated + CHUNK_BYTES;
    assert!(maximum < FRAMES * 2 * 4);
    let sample = fixture.read(start, end, maximum).unwrap();
    fixture.assert_bits(&sample, start, end);
    assert!(!Arc::ptr_eq(&sample.samples, &fixture.reference.samples));
    assert_eq!(
        window_read_observation_for_test(),
        WindowReadObservation {
            read_bytes: allocated as u64,
            allocated_bytes: allocated,
            start_frame: start,
            end_frame: end,
        }
    );
    assert!(fixture.lease.live_cached_pcm_samples().is_none());
}

#[test]
fn identical_range_shares_actual_pcm_arc_and_changes_only_view_revision() {
    let fixture = Fixture::new();
    let held = fixture.reference.samples.len() * 4;
    let sample = fixture.read(11, 27, held).unwrap();
    assert!(Arc::ptr_eq(&sample.samples, &fixture.reference.samples));
    fixture.assert_bits(&sample, 11, 27);
    assert_eq!(fixture.reference.window_revision(), 2);
    assert!(!Arc::ptr_eq(
        sample.residency.as_ref().unwrap(),
        fixture.reference.residency.as_ref().unwrap(),
    ));
    assert_eq!(
        window_read_observation_for_test(),
        WindowReadObservation {
            start_frame: 11,
            end_frame: 27,
            ..WindowReadObservation::default()
        }
    );
    reset_window_read_observation_for_test();
    assert!(fixture.read(11, 27, held - 1).is_err());
    assert_eq!(
        window_read_observation_for_test(),
        WindowReadObservation::default()
    );
}

#[test]
fn range_budget_is_checked_before_any_new_allocation_or_read() {
    let fixture = Fixture::new();
    let start = 10_000;
    let end = 10_025;
    let peak = fixture.reference.samples.len() * 4 + (end - start) * 2 * 4 + CHUNK_BYTES;
    assert!(fixture.read(start, end, peak - 1).is_err());
    assert_eq!(
        window_read_observation_for_test(),
        WindowReadObservation::default()
    );
    let sample = fixture.read(start, end, peak).unwrap();
    fixture.assert_bits(&sample, start, end);
}

#[test]
fn concurrent_distant_calls_have_independent_file_cursors_and_thread_local_observations() {
    let fixture = Fixture::new();
    let barrier = Arc::new(std::sync::Barrier::new(3));
    let threads = [(29, 117), (FRAMES - 391, FRAMES - 7)].map(|(start, end)| {
        let barrier = barrier.clone();
        let lease = fixture.lease.clone();
        let reference = fixture.reference.clone();
        std::thread::spawn(move || {
            reset_window_read_observation_for_test();
            barrier.wait();
            let sample = lease
                .read_window_cancellable(
                    &reference,
                    start,
                    end,
                    9,
                    ResidentContext::FiniteLoop,
                    PCM_LIMIT_BYTES,
                    &|| false,
                )
                .unwrap();
            (sample, window_read_observation_for_test())
        })
    });
    barrier.wait();
    for (thread, (start, end)) in threads
        .into_iter()
        .zip([(29, 117), (FRAMES - 391, FRAMES - 7)])
    {
        let (sample, observation) = thread.join().unwrap();
        fixture.assert_bits(&sample, start, end);
        assert_eq!(
            observation,
            WindowReadObservation {
                read_bytes: ((end - start) * 2 * 4) as u64,
                allocated_bytes: (end - start) * 2 * 4,
                start_frame: start,
                end_frame: end,
            }
        );
    }
    assert_eq!(
        window_read_observation_for_test(),
        WindowReadObservation::default()
    );
}

#[test]
fn invalid_extent_revision_and_finite_complete_context_fail_before_pcm_mutation() {
    let fixture = Fixture::new();
    for (start, end, revision, context) in [
        (5, 5, 9, ResidentContext::FiniteLoop),
        (7, 6, 9, ResidentContext::FiniteLoop),
        (0, FRAMES + 1, 9, ResidentContext::FiniteLoop),
        (usize::MAX - 1, usize::MAX, 9, ResidentContext::FiniteLoop),
        (2, 9, 0, ResidentContext::FiniteLoop),
        (2, 9, 9, ResidentContext::FullTrack),
        (2, 9, 9, ResidentContext::KeyLockFullTrack),
    ] {
        assert!(
            fixture
                .lease
                .read_window_cancellable(
                    &fixture.reference,
                    start,
                    end,
                    revision,
                    context,
                    PCM_LIMIT_BYTES,
                    &|| false,
                )
                .is_err()
        );
        assert_eq!(
            window_read_observation_for_test(),
            WindowReadObservation::default()
        );
    }
    for context in [
        ResidentContext::FullTrack,
        ResidentContext::KeyLockFullTrack,
    ] {
        let sample = fixture
            .lease
            .read_window_cancellable(
                &fixture.reference,
                0,
                FRAMES,
                9,
                context,
                PCM_LIMIT_BYTES,
                &|| false,
            )
            .unwrap();
        fixture.assert_bits(&sample, 0, FRAMES);
        assert_eq!(sample.residency.as_ref().unwrap().context, context);
    }
}

#[test]
fn cancellation_before_allocation_and_after_first_real_chunk_returns_no_pcm() {
    let fixture = Fixture::new();
    assert!(
        fixture
            .lease
            .read_window_cancellable(
                &fixture.reference,
                0,
                FRAMES,
                9,
                ResidentContext::FullTrack,
                PCM_LIMIT_BYTES,
                &|| true,
            )
            .is_err()
    );
    assert_eq!(
        window_read_observation_for_test(),
        WindowReadObservation::default()
    );
    assert!(
        fixture
            .lease
            .read_window_cancellable(
                &fixture.reference,
                0,
                FRAMES,
                9,
                ResidentContext::FullTrack,
                PCM_LIMIT_BYTES,
                &|| window_read_observation_for_test().read_bytes >= CHUNK_BYTES as u64,
            )
            .is_err()
    );
    let observed = window_read_observation_for_test();
    assert_eq!(observed.read_bytes, CHUNK_BYTES as u64);
    assert_eq!(observed.allocated_bytes, FRAMES * 2 * 4);
    assert!(observed.read_bytes < observed.allocated_bytes as u64);
    assert_eq!(fixture.reference.window_revision(), 2);
    reset_window_read_observation_for_test();
    fixture.assert_bits(&fixture.read(101, 113, PCM_LIMIT_BYTES).unwrap(), 101, 113);
}

#[test]
fn foreign_identity_and_overflowing_reference_are_rejected_without_range_io() {
    let fixture = Fixture::new();
    let source = fixture
        .reference
        .residency
        .as_ref()
        .unwrap()
        .source
        .as_ref();
    let wrong = CompleteSourceIdentity {
        frame_count: source.frame_count,
        channels: source.channels,
        sample_rate_hz: source.sample_rate_hz,
        original_sha256: [99; 32],
        playback_sha256: source.playback_sha256,
        mono_sha256: source.mono_sha256,
        transform_sha256: source.transform_sha256,
        source_zero_frame: source.source_zero_frame,
    };
    let mut foreign = fixture.reference.clone();
    foreign.residency = Some(Arc::new(ResidentSourceView {
        source: Arc::new(wrong),
        start_frame: 11,
        window_revision: 2,
        context: ResidentContext::FiniteLoop,
    }));
    assert!(
        fixture
            .lease
            .read_window_cancellable(
                &foreign,
                11,
                27,
                9,
                ResidentContext::FiniteLoop,
                PCM_LIMIT_BYTES,
                &|| false,
            )
            .is_err()
    );
    let mut overflow = fixture.reference.clone();
    overflow.residency = Some(Arc::new(ResidentSourceView {
        source: fixture.reference.residency.as_ref().unwrap().source.clone(),
        start_frame: usize::MAX - 2,
        window_revision: 2,
        context: ResidentContext::FiniteLoop,
    }));
    assert!(
        fixture
            .lease
            .read_window_cancellable(
                &overflow,
                0,
                5,
                9,
                ResidentContext::FiniteLoop,
                PCM_LIMIT_BYTES,
                &|| false,
            )
            .is_err()
    );
    assert_eq!(
        window_read_observation_for_test(),
        WindowReadObservation::default()
    );
}

#[test]
fn wrong_actual_file_identity_and_full_extent_fail_before_even_shared_view_return() {
    let fixture = Fixture::new();
    let other = Fixture::new();
    let mut lease = fixture.lease.clone();
    lease.cache_path = other.lease.cache_path.clone();
    assert!(
        lease
            .read_window_cancellable(
                &fixture.reference,
                11,
                27,
                9,
                ResidentContext::FiniteLoop,
                PCM_LIMIT_BYTES,
                &|| false,
            )
            .is_err()
    );
    let replacement = fixture._temp.path().join("wrong-extent");
    fs::create_dir(&replacement).unwrap();
    fs::write(replacement.join("playback.f32le"), fixture.encoded(11, 27)).unwrap();
    lease.cache_path = replacement;
    assert!(
        lease
            .read_window_cancellable(
                &fixture.reference,
                11,
                27,
                9,
                ResidentContext::FiniteLoop,
                PCM_LIMIT_BYTES,
                &|| false,
            )
            .is_err()
    );
    assert_eq!(
        window_read_observation_for_test(),
        WindowReadObservation::default()
    );
    assert!(
        fixture
            .lease
            .open_complete_reader(&fixture.reference)
            .is_ok()
    );
}

#[test]
fn range_kernel_preserves_finite_f32_bits_and_rejects_nonfinite_and_eof() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("kernel.f32le");
    let bits = [
        (-0.0_f32).to_bits(),
        0.0_f32.to_bits(),
        f32::MIN_POSITIVE.to_bits(),
        (-1.0_f32).to_bits(),
    ];
    fs::write(
        &path,
        bits.iter()
            .flat_map(|value| value.to_le_bytes())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let mut reader = File::open(&path).unwrap();
    reset_window_read_observation_for_test();
    let samples = warm::read_playback_range(
        &mut reader,
        0,
        bits.len(),
        1,
        CHUNK_BYTES + bits.len() * 4,
        &|| false,
        &mut IntegrityMetrics::default(),
    )
    .unwrap();
    assert_eq!(
        samples
            .iter()
            .map(|sample| sample.to_bits())
            .collect::<Vec<_>>(),
        bits
    );
    drop(reader);
    fs::write(&path, f32::NAN.to_bits().to_le_bytes()).unwrap();
    let mut reader = File::open(&path).unwrap();
    assert!(
        warm::read_playback_range(
            &mut reader,
            0,
            1,
            1,
            CHUNK_BYTES + 4,
            &|| false,
            &mut IntegrityMetrics::default(),
        )
        .is_err()
    );
    assert!(
        warm::read_playback_range(
            &mut reader,
            0,
            2,
            1,
            CHUNK_BYTES + 8,
            &|| false,
            &mut IntegrityMetrics::default(),
        )
        .is_err()
    );
}
