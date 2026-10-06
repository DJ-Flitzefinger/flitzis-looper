//! Non-realtime preparation identity and bounded callback publication checks.

use super::AudioEngine;
use super::constants::NUM_SAMPLES;
use crate::messages::ControlMessage;
use crate::messages::SampleBuffer;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use rtrb::Producer;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Opaque engine-owned admission snapshot. Never constructed from Python numbers.
#[pyclass(frozen)]
pub struct PreparedSourceTicket {
    pub(super) id: usize,
    pub(super) request_id: u64,
    pub(super) sample: SampleBuffer,
    pub(super) sample_rate_hz: u32,
    pub(super) source_version: String,
    pub(super) publication: PreparedSourcePermit,
}

#[pymethods]
impl PreparedSourceTicket {
    /// Bounded callback adoption feedback; polling never drives audio timing.
    pub fn publication_status(&self) -> &'static str {
        match self.publication.status.load(Ordering::Acquire) {
            0 => "captured",
            1 => "pending",
            2 => "accepted",
            _ => "rejected",
        }
    }
}

/// One bounded atomic check at mixer adoption; accepted stems remain source-frame data.
#[derive(Debug, Clone)]
pub(crate) struct PreparedSourcePermit {
    pub(super) epoch: Arc<AtomicU64>,
    pub(super) expected: u64,
    status: Arc<AtomicU8>,
}

impl PreparedSourcePermit {
    pub(super) fn unbound() -> Self {
        Self::new(Arc::new(AtomicU64::new(0)), 1)
    }

    pub(super) fn new(epoch: Arc<AtomicU64>, expected: u64) -> Self {
        Self {
            epoch,
            expected,
            status: Arc::new(AtomicU8::new(0)),
        }
    }

    pub(super) fn mark_pending(&self) -> Result<(), String> {
        self.status
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|_| "prepared source ticket already published".into())
    }

    pub(crate) fn mark_accepted(&self) {
        self.status.store(2, Ordering::Release);
    }

    pub(crate) fn mark_rejected(&self) {
        self.status.store(3, Ordering::Release);
    }
    pub(crate) fn current(&self) -> bool {
        self.epoch.load(Ordering::Acquire) == self.expected
    }

    #[cfg(test)]
    pub(crate) fn for_epoch(epoch: Arc<AtomicU64>, expected: u64) -> Self {
        Self::new(epoch, expected)
    }

    #[cfg(test)]
    pub(crate) fn unrestricted() -> Self {
        Self::for_epoch(Arc::new(AtomicU64::new(1)), 1)
    }
}

pub(super) fn next_epoch(epoch: &AtomicU64) -> Result<u64, String> {
    epoch
        .load(Ordering::Acquire)
        .checked_add(1)
        .ok_or_else(|| "prepared source publication epoch exhausted".into())
}

pub(super) fn validate_prepared_ticket(
    ticket: &PreparedSourceTicket,
    id: usize,
    version: &str,
    request_id: u64,
    epoch: &Arc<AtomicU64>,
    sample: &SampleBuffer,
) -> Result<(), String> {
    if ticket.id != id
        || ticket.request_id != request_id
        || ticket.source_version != version
        || !Arc::ptr_eq(&ticket.publication.epoch, epoch)
        || !ticket.publication.current()
        || ticket.sample.channels != sample.channels
        || !Arc::ptr_eq(&ticket.sample.samples, &sample.samples)
    {
        return Err("stale or foreign prepared source ticket".into());
    }
    Ok(())
}

/// Hash full original bytes off-thread; reject observable changes across the read.
/// This does not replace the later immutable copy-first decoding contract.
pub(super) fn file_sha256(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let before = file.metadata().map_err(|error| error.to_string())?;
    let mut digest = Sha256::new();
    let mut bytes = [0_u8; 64 * 1024];
    let mut count = 0_u64;
    loop {
        let read = file.read(&mut bytes).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        count += read as u64;
        digest.update(&bytes[..read]);
    }
    let after = file.metadata().map_err(|error| error.to_string())?;
    let current = std::fs::metadata(path).map_err(|error| error.to_string())?;
    if count != before.len()
        || before.len() != after.len()
        || after.len() != current.len()
        || before.modified().ok() != after.modified().ok()
        || after.modified().ok() != current.modified().ok()
    {
        return Err("source file changed during content hashing".into());
    }
    Ok(format!("{:x}", digest.finalize()))
}

pub(super) fn version_matches_digest(version: &str, digest: &str) -> bool {
    version
        .rsplit_once("|sha256-v1:")
        .is_some_and(|(path, supplied)| {
            !path.is_empty() && supplied == digest && supplied.len() == 64
        })
}

/// The locked single producer reserves available capacity before invalidating
/// preparation. A consumer can only free slots, so the subsequent push cannot
/// become full. The epoch is visible before the timing command is visible.
pub(super) fn push_preparation_epoch_message<T>(
    producer: &mut Producer<T>,
    message: T,
    epoch: &AtomicU64,
    label: &str,
) -> PyResult<()> {
    let next = next_epoch(epoch).map_err(PyRuntimeError::new_err)?;
    if producer.is_full() {
        return Err(PyRuntimeError::new_err(format!(
            "Failed to send {label} - buffer may be full"
        )));
    }
    epoch.store(next, Ordering::Release);
    producer
        .push(message)
        .map_err(|_| PyRuntimeError::new_err("reserved single-producer capacity lost"))
}

pub(super) fn enqueue_current_prepared_stems(
    engine: &AudioEngine,
    producer: &Arc<Mutex<Producer<ControlMessage>>>,
    ticket: &PreparedSourceTicket,
    source_version: &str,
    stems: crate::messages::PreparedStemSet,
) -> PyResult<()> {
    let id = ticket.id;
    let requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    let cache = engine
        .sample_cache
        .lock()
        .map_err(|_| PyRuntimeError::new_err("sample cache lock poisoned"))?;
    let current = cache[id]
        .as_ref()
        .ok_or_else(|| PyValueError::new_err("sample is not loaded"))?;
    validate_prepared_ticket(
        ticket,
        id,
        source_version,
        requests[id],
        &engine.prepared_source_epochs[id],
        current,
    )
    .map_err(PyValueError::new_err)?;
    let mut producer = producer
        .lock()
        .map_err(|_| PyRuntimeError::new_err("Failed to acquire producer lock"))?;
    if producer.is_full() {
        return Err(PyRuntimeError::new_err(
            "Failed to send PublishPreparedStems - buffer may be full",
        ));
    }
    ticket
        .publication
        .mark_pending()
        .map_err(PyValueError::new_err)?;
    producer
        .push(ControlMessage::PublishPreparedStems { id, stems })
        .map_err(|_| {
            PyRuntimeError::new_err("Failed to send PublishPreparedStems - buffer may be full")
        })
}

pub(super) fn capture_prepared_source(
    engine: &AudioEngine,
    id: usize,
    source_version: String,
) -> PyResult<PreparedSourceTicket> {
    if id >= NUM_SAMPLES {
        return Err(PyValueError::new_err("id out of range"));
    }
    let requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    let cache = engine
        .sample_cache
        .lock()
        .map_err(|_| PyRuntimeError::new_err("sample cache lock poisoned"))?;
    let sample = cache[id]
        .clone()
        .ok_or_else(|| PyValueError::new_err("sample is not loaded"))?;
    let generations = engine
        .loaded_source_generations
        .lock()
        .map_err(|_| PyRuntimeError::new_err("source generation lock poisoned"))?;
    let digests = engine
        .loaded_source_digests
        .lock()
        .map_err(|_| PyRuntimeError::new_err("source digest lock poisoned"))?;
    if !digests[id]
        .as_ref()
        .is_some_and(|digest| version_matches_digest(&source_version, digest))
    {
        return Err(PyValueError::new_err(
            "source content does not match loaded source",
        ));
    }
    if requests[id] == 0 || generations[id].0 == 0 || generations[id].1 == 0 {
        return Err(PyValueError::new_err("loaded source identity unavailable"));
    }
    Ok(PreparedSourceTicket {
        id,
        request_id: requests[id],
        sample,
        sample_rate_hz: generations[id].1,
        source_version,
        publication: PreparedSourcePermit::new(
            engine.prepared_source_epochs[id].clone(),
            engine.prepared_source_epochs[id].load(Ordering::Acquire),
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::super::mixer;
    use super::super::stem_cache::source_version_hash;
    use super::*;
    use crate::messages::ControlParameterMessage;
    fn prepared_test_engine() -> (AudioEngine, String, SampleBuffer) {
        let engine = AudioEngine::new().unwrap();
        let sample = SampleBuffer {
            channels: 1,
            samples: Arc::from([0.25_f32; 16].as_slice()),
        };
        engine.sample_cache.lock().unwrap()[0] = Some(sample.clone());
        engine.pad_request_ids.lock().unwrap()[0] = 7;
        engine.loaded_source_generations.lock().unwrap()[0] = (7, 44_100);
        let digest = "a".repeat(64);
        engine.loaded_source_digests.lock().unwrap()[0] = Some(digest.clone());
        (engine, format!("samples/a.wav|sha256-v1:{digest}"), sample)
    }

    fn prepared_test_set(ticket: &PreparedSourceTicket) -> crate::messages::PreparedStemSet {
        crate::messages::PreparedStemSet {
            reference_samples: ticket.sample.samples.clone(),
            publication: ticket.publication.clone(),
            source_version_hash: source_version_hash(&ticket.source_version),
            sample_rate_hz: ticket.sample_rate_hz,
            channels: 1,
            frame_count: 16,
            available_mask: 31,
            stems: std::array::from_fn(|_| ticket.sample.clone()),
        }
    }

    #[test]
    fn productive_prepared_enqueue_and_callback_report_actual_adoption_once() {
        let (engine, version, sample) = prepared_test_engine();
        let ticket = engine.capture_prepared_source(0, version.clone()).unwrap();
        let (producer, mut consumer) = rtrb::RingBuffer::new(1);
        let producer = Arc::new(Mutex::new(producer));
        producer
            .lock()
            .unwrap()
            .push(ControlMessage::Ping())
            .unwrap();
        assert!(
            enqueue_current_prepared_stems(
                &engine,
                &producer,
                &ticket,
                &version,
                prepared_test_set(&ticket)
            )
            .is_err()
        );
        assert_eq!(ticket.publication_status(), "captured");
        consumer.pop().unwrap();
        enqueue_current_prepared_stems(
            &engine,
            &producer,
            &ticket,
            &version,
            prepared_test_set(&ticket),
        )
        .unwrap();
        assert_eq!(ticket.publication_status(), "pending");
        let ControlMessage::PublishPreparedStems { id, stems } = consumer.pop().unwrap() else {
            panic!("publication");
        };
        let mut mixer = mixer::RtMixer::new(1, 44_100.0);
        mixer.load_sample(id, sample);
        assert!(mixer.publish_prepared_stems(id, stems));
        assert_eq!(ticket.publication_status(), "accepted");
        assert!(
            enqueue_current_prepared_stems(
                &engine,
                &producer,
                &ticket,
                &version,
                prepared_test_set(&ticket)
            )
            .is_err()
        );
        assert!(consumer.pop().is_err());
    }

    #[test]
    fn production_capture_and_enqueue_reject_content_foreign_and_replaced_sources() {
        let (engine, version, _) = prepared_test_engine();
        assert!(
            engine
                .capture_prepared_source(0, "samples/a.wav|1|2".into())
                .is_err()
        );
        assert!(
            engine
                .capture_prepared_source(0, format!("samples/a.wav|sha256-v1:{}", "b".repeat(64)))
                .is_err()
        );
        let ticket = engine.capture_prepared_source(0, version.clone()).unwrap();
        let (other, _, _) = prepared_test_engine();
        let (producer, mut consumer) = rtrb::RingBuffer::new(1);
        let producer = Arc::new(Mutex::new(producer));
        assert!(
            enqueue_current_prepared_stems(
                &other,
                &producer,
                &ticket,
                &version,
                prepared_test_set(&ticket)
            )
            .is_err()
        );
        // Identical values and shape, distinct immutable source owner.
        engine.sample_cache.lock().unwrap()[0] = Some(SampleBuffer {
            channels: 1,
            samples: Arc::from([0.25_f32; 16].as_slice()),
        });
        assert!(
            enqueue_current_prepared_stems(
                &engine,
                &producer,
                &ticket,
                &version,
                prepared_test_set(&ticket)
            )
            .is_err()
        );
        assert_eq!(ticket.publication_status(), "captured");
        assert!(consumer.pop().is_err());
    }

    #[test]
    fn queued_preparation_reports_late_timing_rejection_from_callback() {
        let (engine, version, sample) = prepared_test_engine();
        let ticket = engine.capture_prepared_source(0, version.clone()).unwrap();
        let (producer, mut consumer) = rtrb::RingBuffer::new(1);
        let producer = Arc::new(Mutex::new(producer));
        enqueue_current_prepared_stems(
            &engine,
            &producer,
            &ticket,
            &version,
            prepared_test_set(&ticket),
        )
        .unwrap();
        let (mut parameters, mut parameter_consumer) = rtrb::RingBuffer::new(1);
        push_preparation_epoch_message(
            &mut parameters,
            ControlParameterMessage::SetPadBpm {
                id: 0,
                bpm: Some(120.0),
            },
            &engine.prepared_source_epochs[0],
            "SetPadBpm",
        )
        .unwrap();
        assert!(parameter_consumer.pop().is_ok());
        let ControlMessage::PublishPreparedStems { id, stems } = consumer.pop().unwrap() else {
            panic!("publication");
        };
        let mut mixer = mixer::RtMixer::new(1, 44_100.0);
        mixer.load_sample(id, sample);
        assert!(!mixer.publish_prepared_stems(id, stems));
        assert_eq!(ticket.publication_status(), "rejected");
    }

    #[test]
    fn queued_stems_reject_real_offline_request_admission_and_cancellation() {
        for cancel_existing in [false, true] {
            let (engine, version, sample) = prepared_test_engine();
            let begin = || {
                engine.offline_jobs.begin(
                    0,
                    sample.clone(),
                    44_100,
                    7,
                    super::super::analysis_jobs::OfflineRequestOwner {
                        request_ids: engine.pad_request_ids.clone(),
                        prepared_epoch: engine.prepared_source_epochs[0].clone(),
                    },
                    engine.loader_tx.clone(),
                )
            };
            let existing_job = cancel_existing.then(|| begin().unwrap());
            let ticket = engine.capture_prepared_source(0, version.clone()).unwrap();
            let (producer, mut consumer) = rtrb::RingBuffer::new(1);
            let producer = Arc::new(Mutex::new(producer));
            enqueue_current_prepared_stems(
                &engine,
                &producer,
                &ticket,
                &version,
                prepared_test_set(&ticket),
            )
            .unwrap();
            let before_request = engine.pad_request_ids.lock().unwrap()[0];
            let before_epoch = engine.prepared_source_epochs[0].load(Ordering::Acquire);
            let admitted_job = if let Some(job) = existing_job {
                job.cancel();
                job
            } else {
                begin().unwrap()
            };
            assert_eq!(
                engine.pad_request_ids.lock().unwrap()[0],
                before_request + 1
            );
            assert_eq!(
                engine.prepared_source_epochs[0].load(Ordering::Acquire),
                before_epoch + 1
            );
            let ControlMessage::PublishPreparedStems { id, stems } = consumer.pop().unwrap() else {
                panic!("publication");
            };
            let mut mixer = mixer::RtMixer::new(1, 44_100.0);
            mixer.load_sample(id, sample);
            assert!(!mixer.publish_prepared_stems(id, stems));
            assert_eq!(ticket.publication_status(), "rejected");
            admitted_job.cancel();
        }
    }

    #[test]
    fn timing_epoch_publication_is_fail_closed_and_equal_edits_invalidate() {
        let epoch = AtomicU64::new(1);
        let (mut producer, mut consumer) = rtrb::RingBuffer::new(1);
        let message = ControlParameterMessage::SetPadBpm {
            id: 0,
            bpm: Some(120.0),
        };
        push_preparation_epoch_message(&mut producer, message, &epoch, "SetPadBpm").unwrap();
        assert_eq!(epoch.load(Ordering::Acquire), 2);
        assert!(
            push_preparation_epoch_message(&mut producer, message, &epoch, "SetPadBpm").is_err()
        );
        assert_eq!(epoch.load(Ordering::Acquire), 2);
        consumer.pop().unwrap();
        push_preparation_epoch_message(&mut producer, message, &epoch, "SetPadBpm").unwrap();
        assert_eq!(epoch.load(Ordering::Acquire), 3);
        consumer.pop().unwrap();
        epoch.store(u64::MAX, Ordering::Release);
        assert!(
            push_preparation_epoch_message(&mut producer, message, &epoch, "SetPadBpm").is_err()
        );
        assert!(consumer.pop().is_err());
        assert_eq!(epoch.load(Ordering::Acquire), u64::MAX);
    }
}
