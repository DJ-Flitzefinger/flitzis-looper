//! Cancel the original captured worker at a real positioned component read.
//! The outer existing lane waits for the nested productive Work to be terminal.

use super::*;
use crate::audio_engine::stem_pair::{
    PairReadObservation, range_observation_for_test, reset_range_observation_for_test,
    set_range_read_observer_for_test,
};

const FRAMES: usize = 20_003;
const READ_CHUNK_BYTES: usize = 64 * 1024;
const ID: usize = 1;

struct LargeSource {
    version: String,
    wav_reference: String,
    wav_path: PathBuf,
    original: PathBuf,
    original_bytes: Vec<u8>,
    _original_owner: crate::audio_engine::project_assets::ProjectAssetLease,
}

fn adopt_large_source(h: &mut Harness) -> LargeSource {
    let incoming = h.root.join("component-cancellation.wav");
    let encoded = stereo_wav(FRAMES);
    fs::write(&incoming, &encoded).unwrap();
    let material = prepare_material(&h.root, &incoming, RATE, 2, &|| false).unwrap();
    assert_eq!(material.sample.frame_count(), FRAMES);
    let version = canonical_version(&material);
    let wav_reference = canonical_wav_reference(&material);
    let wav_path = h.root.parent().unwrap().join(&wav_reference);
    write_complete_wavs(&wav_path, &encoded, &version);
    let wav_path = fs::canonicalize(wav_path).unwrap();
    let original = material.lease.original_path.clone();
    let original_bytes = fs::read(&original).unwrap();
    let original_owner = h.engine.project_assets.acquire(&h.root, &original).unwrap();
    let preparation = control::prepared_for_test(&h.engine, material, &h.root).unwrap();
    let adopted = control::adopt_for_format(
        &h.engine,
        ID,
        &preparation,
        h.producer.clone(),
        (2, RATE, h.root.clone()),
    )
    .unwrap();
    wait_until(|| h.consumer.peek().is_ok());
    assert_eq!(h.callback.drain(&mut h.consumer), 1);
    assert!(matches!(
        terminal(&h.engine, adopted.request_id()),
        LoaderEvent::Success { id: ID, .. }
    ));
    wait_until(|| h.engine.cold_loading[ID].load(Ordering::Acquire) == 0);
    assert!(adopted.is_current().unwrap());
    preparation.release_preparation().unwrap();
    LargeSource {
        version,
        wav_reference,
        wav_path,
        original,
        original_bytes,
        _original_owner: original_owner,
    }
}

struct RangeObserverReset;
impl Drop for RangeObserverReset {
    fn drop(&mut self) {
        set_range_read_observer_for_test(None);
    }
}

struct ObservedWork {
    result: Result<PreparedStemPair, String>,
    ticket: PreparedSourceTicket,
    reads: PairReadObservation,
    read_calls: usize,
}

fn run_captured_work(h: &Harness, source: &LargeSource, cancel_first_read: bool) -> ObservedWork {
    let ticket = h
        .engine
        .capture_prepared_source(ID, source.version.clone())
        .unwrap();
    assert!(ticket.publication.current());
    let component_bytes = ticket.sample.samples.len() * 4;
    assert!(component_bytes > READ_CHUNK_BYTES * 2);
    let start_bytes = ticket.sample.resident_start() * ticket.sample.channels * 4;
    let work = h
        .engine
        .capture_stem_pair_work(&h.root, ID, &source.version, &ticket)
        .unwrap();
    let publication = ticket.publication.clone();
    let wav_reference = source.wav_reference.clone();
    let read_calls = Arc::new(AtomicUsize::new(0));
    let hook_calls = read_calls.clone();
    // This is the existing owned lane. The nested real Work detects that same
    // lane and stays inline; original-ticket cancellation cannot make this
    // outer, independent waiter return before the actual preparation ends.
    let (result, reads) = h
        .engine
        .cold_jobs
        .handle()
        .execute_owned(
            || false,
            move || {
                let _reset = RangeObserverReset;
                reset_range_observation_for_test();
                set_range_read_observer_for_test(Some(Box::new(move |index, start, bytes| {
                    if hook_calls.fetch_add(1, Ordering::AcqRel) == 0 {
                        assert_eq!((index, start, bytes), (0, start_bytes, READ_CHUNK_BYTES));
                        if cancel_first_read {
                            assert!(publication.cancel_unclaimed());
                            assert!(!publication.current());
                        }
                    }
                })));
                let result = work.prepare(&wav_reference, true, None, None);
                Ok((result, range_observation_for_test()))
            },
        )
        .unwrap();
    ObservedWork {
        result,
        ticket,
        reads,
        read_calls: read_calls.load(Ordering::Acquire),
    }
}

fn resident_counts(h: &Harness) -> (usize, usize, usize, usize) {
    let snapshot = h
        .engine
        .project_assets
        .resident_budget_status(&h.engine.resident_account)
        .unwrap();
    (
        snapshot.held_unique_pcm_bytes,
        snapshot.pending_resident_bytes,
        snapshot.unique_backing_count,
        snapshot.pending_reservation_count,
    )
}

#[test]
fn ordinary_stem_work_cancelled_at_first_component_chunk_stops_reads_and_registration() {
    let mut h = Harness::new();
    let source = adopt_large_source(&mut h);
    let before_resident = resident_counts(&h);
    let before_readers = h.engine.project_assets.status().unwrap().1;
    let observed = run_captured_work(&h, &source, true);
    assert_eq!(observed.ticket.publication_status(), "rejected");
    assert!(!observed.ticket.publication.current());
    let shared = h
        .engine
        .project_assets
        .shared_stems(
            &source.wav_path,
            &observed.ticket.sample,
            &source.version,
            RATE,
        )
        .unwrap();
    // Keep an unexpected successful result alive until every observation is
    // captured, so dropping its component Arcs cannot hide reader registration.
    let cancelled = observed
        .result
        .as_ref()
        .err()
        .is_some_and(|error| error.contains("cancelled"));
    assert_eq!(
        (
            cancelled,
            observed.reads.read_bytes,
            observed.reads.allocation_attempts,
            observed.reads.allocated_bytes,
            observed.read_calls,
            shared.is_some(),
            h.engine.project_assets.status().unwrap().1,
            resident_counts(&h),
        ),
        (
            true,
            [READ_CHUNK_BYTES as u64, 0, 0, 0, 0],
            [1, 0, 0, 0, 0],
            0,
            1,
            false,
            before_readers,
            before_resident,
        ),
        "original Work must stop after its first real read; error={:?}; ranges={:?}",
        observed.result.as_ref().err(),
        observed.reads.byte_ranges
    );
    assert!(h.consumer.peek().is_err());
    assert_eq!(fs::read(&source.original).unwrap(), source.original_bytes);
}

#[test]
fn ordinary_stem_work_without_cancellation_reads_exact_four_components_and_registers_them() {
    let mut h = Harness::new();
    let source = adopt_large_source(&mut h);
    let observed = run_captured_work(&h, &source, false);
    let pair = observed.result.unwrap();
    let component_bytes = observed.ticket.sample.samples.len() * 4;
    let start_bytes = observed.ticket.sample.resident_start() * 2 * 4;
    let range = Some((start_bytes, start_bytes + component_bytes));
    assert_eq!(
        observed.reads.read_bytes,
        [
            component_bytes as u64,
            component_bytes as u64,
            component_bytes as u64,
            component_bytes as u64,
            0,
        ]
    );
    assert_eq!(
        observed.reads.byte_ranges,
        [range, range, range, range, None]
    );
    assert_eq!(observed.reads.allocation_attempts, [1, 1, 1, 1, 0]);
    assert_eq!(observed.reads.allocated_bytes, component_bytes * 4);
    assert_eq!(
        observed.read_calls,
        component_bytes.div_ceil(READ_CHUNK_BYTES) * 4
    );
    let signed = [0_i16, 8192, -16384, 32767, 0, -4096, 16384];
    let expected: Vec<f32> = (observed.ticket.sample.resident_start()
        ..observed.ticket.sample.resident_end())
        .flat_map(|frame| [f32::from(signed[frame % signed.len()]) / f32::from(i16::MAX); 2])
        .collect();
    let shared = h
        .engine
        .project_assets
        .shared_stems(
            &source.wav_path,
            &observed.ticket.sample,
            &source.version,
            RATE,
        )
        .unwrap()
        .expect("uncancelled actual Work registered its completed component readers");
    for (component, registered) in pair
        .component_views_for_test()
        .unwrap()
        .iter()
        .zip(&shared.stems)
    {
        assert_eq!(component.samples.len(), expected.len());
        let mismatch = component
            .samples
            .iter()
            .zip(&expected)
            .position(|(actual, expected)| actual.to_bits() != expected.to_bits());
        assert_eq!(
            mismatch, None,
            "complete component PCM must match at every sample"
        );
        assert!(component.same_window(&observed.ticket.sample));
        assert!(Arc::ptr_eq(&component.samples, &registered.samples));
    }
    assert!(observed.ticket.publication.current());
    assert_eq!(observed.ticket.publication_status(), "captured");
    assert!(h.consumer.peek().is_err());
    assert_eq!(fs::read(&source.original).unwrap(), source.original_bytes);
}
