//! Finite source-window preparation shares the two-worker, 32-job cold lane.
//! Complete file readers and all large allocations/destruction remain off callback.
use super::AudioEngine;
use super::cold_store::CommittedColdLease;
use super::constants::NUM_SAMPLES;
use super::input_runtime_binding::{self, InputRuntimePadBinding};
use super::prepared_source::PreparedSourcePermit;
use crate::messages::{ControlMessage, PreparedStemSet, ResidentContext, SampleBuffer};
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use rtrb::Producer;
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU8, Ordering},
};
use std::time::{Duration, Instant};

const ADOPTION_DEADLINE: Duration = Duration::from_secs(30);

#[derive(Clone)]
struct StemOwner {
    set: PreparedStemSet,
    source_version: String,
    cache_dir: PathBuf,
    generation_path: PathBuf,
}

struct PendingWindow {
    sample: Option<SampleBuffer>,
    stems: Option<PreparedStemSet>,
    publication: PreparedSourcePermit,
    expected_revision: u64,
    token: Arc<()>,
    state: Arc<WindowState>,
    admitted: Instant,
}

#[derive(Default)]
pub(super) struct ResidentStemCache {
    accepted: Option<StemOwner>,
    pending: Option<StemOwner>,
    relocation: Option<PendingWindow>,
}

#[derive(Default)]
struct WindowState {
    terminal: AtomicU8,
    error: Mutex<Option<String>>,
}

impl WindowState {
    fn fail(&self, error: String, cancelled: bool) {
        if let Ok(mut message) = self.error.lock() {
            *message = Some(error);
        }
        self.terminal
            .store(if cancelled { 2 } else { 1 }, Ordering::Release);
    }
}

/// Destroying an unrun queued job (including shutdown) settles its public state.
struct JobGuard {
    publication: PreparedSourcePermit,
    state: Arc<WindowState>,
    enqueued: bool,
}

impl JobGuard {
    fn mark_enqueued(&mut self) {
        self.enqueued = true;
    }
}

impl Drop for JobGuard {
    fn drop(&mut self) {
        if !self.enqueued
            && self.publication.cancel_unclaimed()
            && self.state.terminal.load(Ordering::Acquire) == 0
        {
            self.state.fail(
                "resident preparation cancelled before native enqueue".into(),
                true,
            );
        }
    }
}

/// Native ACK is authoritative; polling cannot cause callback adoption.
#[pyclass(frozen)]
pub struct ResidentWindowTicket {
    publication: PreparedSourcePermit,
    state: Arc<WindowState>,
    #[pyo3(get)]
    window_revision: u64,
    #[pyo3(get)]
    start_frame: usize,
    #[pyo3(get)]
    end_frame: usize,
    #[pyo3(get)]
    full_frame_count: usize,
}

#[pymethods]
impl ResidentWindowTicket {
    pub fn publication_status(&self) -> &'static str {
        match self.state.terminal.load(Ordering::Acquire) {
            1 => "failed",
            2 => "cancelled",
            _ => match self.publication.status() {
                "captured" => "preparing",
                status => status,
            },
        }
    }

    pub fn error(&self) -> PyResult<Option<String>> {
        self.state
            .error
            .lock()
            .map(|error| error.clone())
            .map_err(|_| PyRuntimeError::new_err("resident error lock poisoned"))
    }

    /// A claimed callback tail cannot be cancelled or rolled back by metadata.
    pub fn cancel(&self) -> bool {
        let cancelled = self.publication.cancel_unclaimed();
        if cancelled {
            self.state
                .fail("resident preparation cancelled".into(), true);
        }
        cancelled
    }
}

impl AudioEngine {
    pub(super) fn record_stems(
        &self,
        id: usize,
        set: PreparedStemSet,
        source_version: String,
        cache_dir: PathBuf,
        generation_path: PathBuf,
    ) -> PyResult<()> {
        let mut owners = self
            .resident_stem_cache
            .lock()
            .map_err(|_| PyRuntimeError::new_err("resident stem ownership lock poisoned"))?;
        owners[id].pending = Some(StemOwner {
            set,
            source_version,
            cache_dir,
            generation_path,
        });
        Ok(())
    }
}

pub(super) fn reconcile(engine: &AudioEngine) -> PyResult<()> {
    let _requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    reconcile_under_request_lock(engine)
}

pub(super) fn cancel_all(engine: &AudioEngine) -> PyResult<()> {
    let owners = engine
        .resident_stem_cache
        .lock()
        .map_err(|_| PyRuntimeError::new_err("resident ownership lock poisoned"))?;
    for owner in owners.iter() {
        if let Some(pending) = &owner.relocation
            && pending.publication.cancel_unclaimed()
        {
            pending
                .state
                .fail("resident preparation cancelled by shutdown".into(), true);
        }
    }
    Ok(())
}

pub(super) fn reconcile_under_request_lock(engine: &AudioEngine) -> PyResult<()> {
    let mut cache = engine
        .sample_cache
        .lock()
        .map_err(|_| PyRuntimeError::new_err("sample cache lock poisoned"))?;
    let mut owners = engine
        .resident_stem_cache
        .lock()
        .map_err(|_| PyRuntimeError::new_err("resident stem ownership lock poisoned"))?;
    for id in 0..NUM_SAMPLES {
        let owner = &mut owners[id];
        if let Some(pending) = owner.pending.as_ref() {
            match pending.set.publication.status() {
                "accepted" => owner.accepted = owner.pending.take(),
                "rejected" => {
                    owner.pending = None;
                }
                _ => {}
            }
        }
        if let Some(pending) = owner.relocation.as_ref() {
            if pending.admitted.elapsed() >= ADOPTION_DEADLINE
                && pending.publication.cancel_unclaimed()
            {
                pending
                    .state
                    .fail("resident native adoption deadline expired".into(), true);
            }
            let status = pending.publication.status();
            if status == "accepted" {
                let current = pending.sample.as_ref().is_some_and(|next| {
                    cache[id].as_ref().is_some_and(|sample| {
                        sample.same_source(next)
                            && sample.window_revision() == pending.expected_revision
                    })
                });
                let pending = owner.relocation.take().expect("checked pending window");
                if current {
                    cache[id] = pending.sample;
                    if let Some(accepted) = &mut owner.accepted
                        && let Some(stems) = pending.stems
                    {
                        accepted.set = stems;
                    }
                }
            } else if status == "rejected" {
                owner.relocation = None;
            }
        }
        if owner.accepted.as_ref().is_some_and(|accepted| {
            !cache[id].as_ref().is_some_and(|sample| {
                accepted
                    .set
                    .stems
                    .first()
                    .is_some_and(|stem| sample.same_source(stem))
            })
        }) {
            owner.accepted = None;
        }
    }
    Ok(())
}

struct WindowWork {
    id: usize,
    old: SampleBuffer,
    start: usize,
    end: usize,
    revision: u64,
    context: ResidentContext,
    request: u64,
    binding: InputRuntimePadBinding,
    source_lease: CommittedColdLease,
    stem_owner: Option<StemOwner>,
    requests: Arc<Mutex<Vec<u64>>>,
    cache: Arc<Mutex<Vec<Option<SampleBuffer>>>>,
    owners: Arc<Mutex<Vec<ResidentStemCache>>>,
    assets: Arc<super::project_assets::ProjectAssets>,
    producer: Arc<Mutex<Producer<ControlMessage>>>,
    publication: PreparedSourcePermit,
    state: Arc<WindowState>,
    token: Arc<()>,
    cancelled: Arc<std::sync::atomic::AtomicBool>,
}

impl WindowWork {
    fn current(&self) -> bool {
        !self.cancelled.load(Ordering::Acquire)
            && self.publication.current()
            && self.binding.current()
    }

    fn prepare(&self) -> Result<(), String> {
        if !self.current() {
            return Err("resident source/window was superseded".into());
        }
        let frames = self.old.frame_count();
        let full_bytes = frames
            .checked_mul(self.old.channels)
            .and_then(|n| n.checked_mul(4))
            .ok_or("resident PCM geometry overflow")?;
        let window_bytes = (self.end - self.start)
            .checked_mul(self.old.channels)
            .and_then(|n| n.checked_mul(4))
            .ok_or("resident PCM geometry overflow")?;
        let peak = if self.stem_owner.is_some() {
            // New finite fullmix persists while complete stems are read/aligned/cropped.
            super::stem_cache::admitted_stem_pcm_bytes(frames, self.old.channels)?
                .checked_add(window_bytes)
                .ok_or("resident PCM overlap overflow")?
        } else {
            full_bytes
                .checked_mul(2)
                .and_then(|n| n.checked_add(window_bytes))
                .ok_or("resident PCM overlap overflow")?
        };
        if peak > super::cold_jobs::PCM_LIMIT_BYTES {
            return Err("resident preparation exceeds the 1-GiB transient PCM admission".into());
        }
        let complete = if self.old.resident_start() == 0 && self.old.resident_end() == frames {
            self.old.clone()
        } else {
            self.source_lease
                .read_complete_cancellable(&self.old, super::cold_jobs::PCM_LIMIT_BYTES, &|| {
                    !self.current()
                })
                .map_err(|error| error.to_string())?
        };
        self.assets
            .retain_cold_reader(&self.source_lease, &complete)
            .map_err(|error| error.to_string())?;
        if !self.current() {
            return Err("resident preparation cancelled after complete read".into());
        }
        let sample = complete.window(self.start, self.end, self.revision, self.context)?;
        self.assets
            .retain_cold_reader(&self.source_lease, &sample)
            .map_err(|error| error.to_string())?;
        let stems = self
            .stem_owner
            .as_ref()
            .map(|owner| -> Result<PreparedStemSet, String> {
                let path = owner.cache_dir.to_str().ok_or("invalid stem cache path")?;
                let mut set = super::stem_cache::prepare_stem_buffers_from_cache(
                    &owner.source_version,
                    &complete,
                    self.binding.binding.sample_rate_hz,
                    path,
                )?;
                if *set.complete_set_identity != *owner.set.complete_set_identity {
                    return Err("complete accepted stem set changed".into());
                }
                set.complete_set_identity = owner.set.complete_set_identity.clone();
                set.accepted_timing = self.binding.binding.accepted;
                set.publication = owner.set.publication.clone();
                let set = set.window_for(&sample)?;
                self.assets
                    .retain_stems(owner.generation_path.clone(), &set)
                    .map_err(|error| error.to_string())?;
                Ok(set)
            })
            .transpose()?;
        drop(complete);
        let requests = self.requests.lock().map_err(|_| "request lock poisoned")?;
        let cache = self
            .cache
            .lock()
            .map_err(|_| "sample cache lock poisoned")?;
        if requests[self.id] != self.request
            || !self.current()
            || !cache[self.id]
                .as_ref()
                .is_some_and(|current| current.same_window(&self.old))
        {
            return Err("stale resident source/window preparation".into());
        }
        let mut producer = self.producer.lock().map_err(|_| "producer lock poisoned")?;
        if producer.is_full() {
            return Err("resident command queue is full".into());
        }
        let mut owners = self
            .owners
            .lock()
            .map_err(|_| "resident ownership lock poisoned")?;
        let pending = owners[self.id]
            .relocation
            .as_mut()
            .filter(|pending| Arc::ptr_eq(&pending.token, &self.token))
            .ok_or("resident request no longer owns pending publication")?;
        self.publication.mark_pending()?;
        pending.sample = Some(sample.clone());
        pending.stems = stems.clone();
        producer
            .push(ControlMessage::RelocateResident(Box::new(
                crate::messages::ResidentTransaction {
                    id: self.id,
                    sample,
                    stems,
                    binding: self.binding.binding,
                    publication: self.publication.clone(),
                    expected_window_revision: self.old.window_revision(),
                },
            )))
            .map_err(|_| "reserved single-producer capacity lost")?;
        Ok(())
    }
}

/// Prepare only storage which covers effective active geometry. New geometry and
/// nonresident seeks use the separate finite-readiness control stage.
pub(super) fn relocate(
    engine: &AudioEngine,
    id: usize,
    start_s: f64,
    end_s: f64,
) -> PyResult<ResidentWindowTicket> {
    let handle = engine
        .stream_handle
        .as_ref()
        .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;
    relocate_with_producer(engine, id, start_s, end_s, handle.producer.clone())
}

pub(super) fn relocate_with_producer(
    engine: &AudioEngine,
    id: usize,
    start_s: f64,
    end_s: f64,
    producer: Arc<Mutex<Producer<ControlMessage>>>,
) -> PyResult<ResidentWindowTicket> {
    if id >= NUM_SAMPLES
        || !start_s.is_finite()
        || !end_s.is_finite()
        || start_s < 0.0
        || end_s <= start_s
    {
        return Err(PyValueError::new_err("invalid resident source range"));
    }
    reconcile(engine)?;
    let binding = input_runtime_binding::capture(engine, id)?
        .filter(|current| current.available() && current.current())
        .ok_or_else(|| PyValueError::new_err("current complete source/timing unavailable"))?;
    let reservation = engine
        .cold_jobs
        .reserve()
        .map_err(PyRuntimeError::new_err)?;
    let requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    let cache = engine
        .sample_cache
        .lock()
        .map_err(|_| PyRuntimeError::new_err("sample cache lock poisoned"))?;
    let old = cache[id]
        .clone()
        .ok_or_else(|| PyValueError::new_err("sample is not loaded"))?;
    let view = old
        .residency
        .as_ref()
        .ok_or_else(|| PyValueError::new_err("complete cache residency is unavailable"))?;
    let revision = old
        .window_revision()
        .checked_add(1)
        .ok_or_else(|| PyValueError::new_err("window revision exhausted"))?;
    let rate = binding.binding.sample_rate_hz;
    let (start, end, context) = if view.context == ResidentContext::KeyLockFullTrack {
        (0, old.frame_count(), ResidentContext::KeyLockFullTrack)
    } else {
        let start = (start_s * f64::from(rate)).round() as usize;
        let end = (end_s * f64::from(rate)).round() as usize;
        (
            start,
            end,
            if start == 0 && end == old.frame_count() {
                ResidentContext::FullTrack
            } else {
                ResidentContext::FiniteLoop
            },
        )
    };
    if start >= end || end > old.frame_count() {
        return Err(PyValueError::new_err("resident range exceeds full source"));
    }
    let source_lease = engine
        .cold_leases
        .lock()
        .map_err(|_| PyRuntimeError::new_err("cold lease lock poisoned"))?[id]
        .clone()
        .ok_or_else(|| PyValueError::new_err("complete source lease unavailable"))?;
    let publication = PreparedSourcePermit::new(
        engine.prepared_source_epochs[id].clone(),
        engine.prepared_source_epochs[id].load(Ordering::Acquire),
    );
    let state = Arc::new(WindowState::default());
    let token = Arc::new(());
    let ticket = ResidentWindowTicket {
        publication: publication.clone(),
        state: state.clone(),
        window_revision: revision,
        start_frame: start,
        end_frame: end,
        full_frame_count: old.frame_count(),
    };
    let mut owners = engine
        .resident_stem_cache
        .lock()
        .map_err(|_| PyRuntimeError::new_err("resident ownership lock poisoned"))?;
    if owners[id].pending.is_some() || owners[id].relocation.is_some() {
        return Err(PyValueError::new_err(
            "resident publication is already pending",
        ));
    }
    let stem_owner = owners[id].accepted.clone();
    owners[id].relocation = Some(PendingWindow {
        sample: None,
        stems: None,
        publication: publication.clone(),
        expected_revision: old.window_revision(),
        token: token.clone(),
        state: state.clone(),
        admitted: Instant::now(),
    });
    let work = WindowWork {
        id,
        old,
        start,
        end,
        revision,
        context,
        request: requests[id],
        binding,
        source_lease,
        stem_owner,
        requests: engine.pad_request_ids.clone(),
        cache: engine.sample_cache.clone(),
        owners: engine.resident_stem_cache.clone(),
        assets: engine.project_assets.clone(),
        producer,
        publication: publication.clone(),
        state: state.clone(),
        token,
        cancelled: engine.cold_cancelled.clone(),
    };
    drop(owners);
    drop(cache);
    drop(requests);
    let mut guard = JobGuard {
        publication: publication.clone(),
        state: state.clone(),
        enqueued: false,
    };
    engine
        .cold_jobs
        .submit(reservation, move || match work.prepare() {
            Ok(()) => guard.mark_enqueued(),
            Err(error) => {
                let cancelled = !work.current();
                work.state.fail(error, cancelled);
                work.publication.cancel_unclaimed();
            }
        })
        .map_err(PyRuntimeError::new_err)?;
    Ok(ticket)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Condvar, mpsc};

    fn source() -> SampleBuffer {
        SampleBuffer {
            residency: None,
            channels: 1,
            samples: Arc::from(vec![0.25; 1000]),
        }
        .with_complete_source(48_000)
    }

    #[test]
    fn claimed_callback_state_is_nonterminal_and_cannot_be_cancelled_or_reconciled_early() {
        let engine = AudioEngine::new().unwrap();
        let full = source();
        let old = full
            .window(200, 400, 1, ResidentContext::FiniteLoop)
            .unwrap();
        let next = full
            .window(190, 410, 2, ResidentContext::FiniteLoop)
            .unwrap();
        engine.sample_cache.lock().unwrap()[0] = Some(old.clone());
        let publication = PreparedSourcePermit::unrestricted();
        publication.mark_pending().unwrap();
        let state = Arc::new(WindowState::default());
        engine.resident_stem_cache.lock().unwrap()[0].relocation = Some(PendingWindow {
            sample: Some(next.clone()),
            stems: None,
            publication: publication.clone(),
            expected_revision: 1,
            token: Arc::new(()),
            state: state.clone(),
            admitted: Instant::now(),
        });
        assert!(publication.claim_resident());
        assert_eq!(publication.status(), "adopting");
        assert!(!publication.cancel_unclaimed());
        reconcile(&engine).unwrap();
        assert!(
            engine.resident_stem_cache.lock().unwrap()[0]
                .relocation
                .is_some()
        );
        assert!(
            engine.sample_cache.lock().unwrap()[0]
                .as_ref()
                .unwrap()
                .same_window(&old)
        );
        publication.mark_accepted();
        reconcile(&engine).unwrap();
        assert!(
            engine.sample_cache.lock().unwrap()[0]
                .as_ref()
                .unwrap()
                .same_window(&next)
        );
        assert!(
            engine.resident_stem_cache.lock().unwrap()[0]
                .relocation
                .is_none()
        );
        assert_eq!(state.terminal.load(Ordering::Acquire), 0);
    }

    #[test]
    fn timeout_and_shutdown_revoke_unclaimed_pending_and_preserve_previous_effective_source() {
        for shutdown in [false, true] {
            let engine = AudioEngine::new().unwrap();
            let full = source();
            let old = full
                .window(200, 400, 1, ResidentContext::FiniteLoop)
                .unwrap();
            let next = full
                .window(190, 410, 2, ResidentContext::FiniteLoop)
                .unwrap();
            engine.sample_cache.lock().unwrap()[0] = Some(old.clone());
            let publication = PreparedSourcePermit::unrestricted();
            publication.mark_pending().unwrap();
            let state = Arc::new(WindowState::default());
            engine.resident_stem_cache.lock().unwrap()[0].relocation = Some(PendingWindow {
                sample: Some(next),
                stems: None,
                publication: publication.clone(),
                expected_revision: 1,
                token: Arc::new(()),
                state: state.clone(),
                admitted: if shutdown {
                    Instant::now()
                } else {
                    Instant::now() - ADOPTION_DEADLINE - Duration::from_secs(1)
                },
            });
            if shutdown {
                cancel_all(&engine).unwrap();
            }
            reconcile(&engine).unwrap();
            assert_eq!(publication.status(), "rejected");
            assert_eq!(state.terminal.load(Ordering::Acquire), 2);
            assert!(
                engine.sample_cache.lock().unwrap()[0]
                    .as_ref()
                    .unwrap()
                    .same_window(&old)
            );
            assert!(
                engine.resident_stem_cache.lock().unwrap()[0]
                    .relocation
                    .is_none()
            );
        }
    }

    #[test]
    fn actual_cold_lane_shutdown_drops_queued_job_guard_and_settles_ticket_without_running_it() {
        let mut lane = super::super::cold_jobs::ColdJobs::new().unwrap();
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let (tx, rx) = mpsc::channel();
        for _ in 0..2 {
            let gate = gate.clone();
            let tx = tx.clone();
            lane.submit(lane.reserve().unwrap(), move || {
                tx.send(()).unwrap();
                let mut ready = gate.0.lock().unwrap();
                while !*ready {
                    ready = gate.1.wait(ready).unwrap();
                }
            })
            .unwrap();
        }
        rx.recv_timeout(Duration::from_secs(3)).unwrap();
        rx.recv_timeout(Duration::from_secs(3)).unwrap();
        let publication = PreparedSourcePermit::unrestricted();
        let state = Arc::new(WindowState::default());
        let mut guard = JobGuard {
            publication: publication.clone(),
            state: state.clone(),
            enqueued: false,
        };
        lane.submit(lane.reserve().unwrap(), move || guard.mark_enqueued())
            .unwrap();
        let shutdown = std::thread::spawn(move || lane.shutdown());
        let deadline = Instant::now() + Duration::from_secs(3);
        while state.terminal.load(Ordering::Acquire) == 0 && Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert_eq!(state.terminal.load(Ordering::Acquire), 2);
        assert_eq!(publication.status(), "rejected");
        *gate.0.lock().unwrap() = true;
        gate.1.notify_all();
        shutdown.join().unwrap();
    }

    #[test]
    fn same_source_ack_cannot_install_after_a_new_assignment_supersedes_the_control_owner() {
        let engine = AudioEngine::new().unwrap();
        let full = source();
        let next = full
            .window(190, 410, 2, ResidentContext::FiniteLoop)
            .unwrap();
        let replacement = source()
            .window(200, 400, 1, ResidentContext::FiniteLoop)
            .unwrap();
        engine.sample_cache.lock().unwrap()[0] = Some(replacement.clone());
        let publication = PreparedSourcePermit::unrestricted();
        publication.mark_pending().unwrap();
        publication.mark_accepted();
        engine.resident_stem_cache.lock().unwrap()[0].relocation = Some(PendingWindow {
            sample: Some(next),
            stems: None,
            publication,
            expected_revision: 1,
            token: Arc::new(()),
            state: Arc::new(WindowState::default()),
            admitted: Instant::now(),
        });
        reconcile(&engine).unwrap();
        assert!(
            engine.sample_cache.lock().unwrap()[0]
                .as_ref()
                .unwrap()
                .same_window(&replacement)
        );
    }
}
