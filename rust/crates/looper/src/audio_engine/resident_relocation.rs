//! Finite source-window preparation shares the two-worker, 32-job cold lane.
//! Complete file readers and all large allocations/destruction remain off callback.
use super::AudioEngine;
use super::cold_store::CommittedColdLease;
use super::constant_timing::CurrentTimingAcknowledgements;
use super::constants::NUM_SAMPLES;
use super::input_runtime_binding::{
    self, InputPadBinding, InputRuntimeOwnership, InputRuntimePadBinding,
};
use super::prepared_source::{PreparedSourcePermit, ResidentLaunchPermit};
use crate::messages::{
    ControlMessage, PreparedStemSet, ResidentContext, ResidentControlIntent, SampleBuffer,
};
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use rtrb::Producer;
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
};
use std::time::{Duration, Instant};

const ADOPTION_DEADLINE: Duration = Duration::from_secs(30);

#[derive(Clone)]
pub(super) struct StemPairOwner {
    pub root: PathBuf,
    pub descriptor_reference: String,
    pub source_lease: CommittedColdLease,
    pub reader: Arc<super::stem_pair::VerifiedStemPair>,
}

#[derive(Clone)]
struct StemOwner {
    set: PreparedStemSet,
    source_version: String,
    cache_dir: PathBuf,
    generation_path: PathBuf,
    pair: Option<StemPairOwner>,
}

struct StemDescriptor {
    identity: std::sync::Weak<[u8; 32]>,
    source_version: String,
    cache_dir: PathBuf,
    generation_path: PathBuf,
    pair: Option<StemPairOwner>,
}

impl StemOwner {
    fn prepare_window(
        &self,
        complete: &SampleBuffer,
        window: &SampleBuffer,
        rate: u32,
        assets: &super::project_assets::ProjectAssets,
        cancelled: &impl Fn() -> bool,
    ) -> Result<PreparedStemSet, String> {
        if let Some(owner) = &self.pair {
            owner
                .source_lease
                .verify_reference(window)
                .map_err(|error| error.to_string())?;
            let pair = &owner.reader;
            if pair.descriptor_reference != owner.descriptor_reference {
                return Err("retained pair descriptor differs from owner".into());
            }
            if pair.descriptor.stem_set_identity
                != self
                    .set
                    .complete_set_identity
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>()
            {
                return Err("complete accepted stem pair changed".into());
            }
            let same_range = self.set.stems.iter().all(|stem| {
                stem.same_source(window)
                    && stem.resident_start() == window.resident_start()
                    && stem.resident_end() == window.resident_end()
            });
            let stems = if same_range {
                self.set
                    .stems
                    .clone()
                    .map(|stem| {
                        stem.window(
                            window.resident_start(),
                            window.resident_end(),
                            window.window_revision(),
                            window.residency.as_ref().unwrap().context,
                        )
                    })
                    .into_iter()
                    .collect::<Result<Vec<_>, _>>()?
                    .try_into()
                    .map_err(|_| "retained component count differs".to_string())?
            } else {
                pair.prepare_component_views_cancellable(window, cancelled)?
            };
            let set = PreparedStemSet {
                complete_set_identity: self.set.complete_set_identity.clone(),
                accepted_timing: self.set.accepted_timing,
                reference_samples: window.samples.clone(),
                publication: self.set.publication.clone(),
                source_version_hash: self.set.source_version_hash,
                sample_rate_hz: rate,
                channels: window.channels,
                frame_count: window.frame_count(),
                available_mask: self.set.available_mask,
                stems,
            };
            assets
                .retain_stem_pair(
                    self.generation_path.clone(),
                    window,
                    &self.source_version,
                    &set.complete_set_identity,
                    Some(&set),
                    pair.clone_pins(owner.root.parent().ok_or("pair project root missing")?)?,
                    owner.source_lease.clone(),
                )
                .map_err(|error| error.to_string())?;
            return Ok(set);
        }
        let path = self.cache_dir.to_str().ok_or("invalid stem cache path")?;
        let mut set = super::stem_cache::prepare_stem_buffers_from_cache(
            &self.source_version,
            complete,
            rate,
            path,
        )?;
        if *set.complete_set_identity != *self.set.complete_set_identity {
            return Err("complete accepted stem set changed".into());
        }
        set.complete_set_identity = self.set.complete_set_identity.clone();
        set.window_for(window)
    }
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
    intent_epoch: Arc<AtomicU64>,
    history: Vec<StemDescriptor>,
}

impl ResidentStemCache {
    fn remember(&mut self, owner: &StemOwner) -> PyResult<()> {
        self.history
            .retain(|entry| entry.identity.strong_count() > 0);
        let identity = Arc::downgrade(&owner.set.complete_set_identity);
        if self
            .history
            .iter()
            .any(|entry| entry.identity.ptr_eq(&identity))
        {
            return Ok(());
        }
        if self.history.len() >= 128 {
            return Err(PyRuntimeError::new_err(
                "retained stem descriptor bound exceeded",
            ));
        }
        self.history.push(StemDescriptor {
            identity,
            source_version: owner.source_version.clone(),
            cache_dir: owner.cache_dir.clone(),
            generation_path: owner.generation_path.clone(),
            pair: owner.pair.clone(),
        });
        Ok(())
    }

    fn owner_for(&self, set: &PreparedStemSet) -> Option<StemOwner> {
        if let Some(owner) = &self.accepted
            && Arc::ptr_eq(&owner.set.complete_set_identity, &set.complete_set_identity)
        {
            let mut owner = owner.clone();
            owner.set = set.clone();
            return Some(owner);
        }
        let identity = Arc::downgrade(&set.complete_set_identity);
        let descriptor = self
            .history
            .iter()
            .find(|entry| entry.identity.ptr_eq(&identity))?;
        Some(StemOwner {
            set: set.clone(),
            source_version: descriptor.source_version.clone(),
            cache_dir: descriptor.cache_dir.clone(),
            generation_path: descriptor.generation_path.clone(),
            pair: descriptor.pair.clone(),
        })
    }
}

#[derive(Default)]
struct WindowState {
    terminal: AtomicU8,
    error: Mutex<Option<String>>,
    adopted_binding: Mutex<Option<InputPadBinding>>,
    launch_cancelled: Arc<AtomicBool>,
    #[cfg(test)]
    terminal_observation: Mutex<Option<terminal_order_tests::TerminalObservation>>,
    #[cfg(test)]
    range_observation: Mutex<Option<WindowReadObservation>>,
}

#[cfg(test)]
#[derive(Clone, Debug)]
pub(super) struct WindowReadObservation {
    pub source: super::cold_store::WindowReadObservation,
    pub stems: super::stem_pair::PairReadObservation,
    pub admitted_peak_bytes: usize,
}

impl WindowState {
    /// Settle this exact control intent before publishing terminal failure.
    /// This scalar CAS neither clears a replacement intent nor retires job/read pins.
    fn fail(
        &self,
        ownership: &InputRuntimeOwnership,
        id: usize,
        expected: u64,
        error: String,
        cancelled: bool,
    ) {
        ownership.finish_resident_control(id, expected);
        if let Ok(mut message) = self.error.lock() {
            *message = Some(error);
        }
        self.terminal
            .store(if cancelled { 2 } else { 1 }, Ordering::Release);
        #[cfg(test)]
        if let Some(observation) = self.terminal_observation.lock().unwrap().take() {
            observation.observe();
        }
    }
}

/// Destroying an unrun queued job (including shutdown) settles its public state.
struct JobGuard {
    publication: PreparedSourcePermit,
    state: Arc<WindowState>,
    id: usize,
    ownership: Arc<InputRuntimeOwnership>,
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
                &self.ownership,
                self.id,
                self.publication.expected,
                "resident preparation cancelled before native enqueue".into(),
                true,
            );
        }
    }
}

/// Native ACK is authoritative; polling cannot cause callback adoption.
#[derive(Clone, Debug)]
pub(crate) struct ResidentLaunchGuard {
    // Resident permits contain only fixed atomic Arcs, never PCM,
    // file/evidence leases or collections. Scheduling clones allocate nothing;
    // rejection/execution drops only this bounded scalar metadata.
    publication: ResidentLaunchPermit,
    cancelled: Arc<AtomicBool>,
}

impl PartialEq for ResidentLaunchGuard {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.publication.epoch, &other.publication.epoch)
            && self.publication.expected == other.publication.expected
            && Arc::ptr_eq(&self.cancelled, &other.cancelled)
    }
}

impl ResidentLaunchGuard {
    pub(crate) fn current(&self) -> bool {
        !self.cancelled.load(Ordering::Acquire) && self.publication.current_accepted()
    }
}

#[pyclass(frozen)]
pub struct ResidentWindowTicket {
    publication: PreparedSourcePermit,
    state: Arc<WindowState>,
    #[pyo3(get)]
    window_revision: u64,
    #[pyo3(get)]
    previous_window_revision: u64,
    #[pyo3(get)]
    start_frame: usize,
    #[pyo3(get)]
    end_frame: usize,
    #[pyo3(get)]
    full_frame_count: usize,
    id: usize,
    source_generation: u64,
    binding: InputPadBinding,
    ownership: Arc<InputRuntimeOwnership>,
    acknowledgements: Arc<CurrentTimingAcknowledgements>,
    loop_region: Option<(f64, Option<f64>)>,
}

impl ResidentWindowTicket {
    #[cfg(test)]
    pub(super) fn read_observation_for_test(&self) -> Option<WindowReadObservation> {
        self.state.range_observation.lock().unwrap().clone()
    }
    pub(super) fn launch_message(
        &self,
        exclusive: bool,
        received_at_ns: u64,
    ) -> Option<ControlMessage> {
        if self.publication.status() != "accepted"
            || !self.is_current()
            || self.state.launch_cancelled.load(Ordering::Acquire)
        {
            return None;
        }
        let (start_s, end_s) = self.loop_region?;
        let launch_revision = self.ownership.launch_revision(self.id);
        if !self.ownership.launch_current(self.id, launch_revision) {
            return None;
        }
        let binding = if self.publication.preserved_window() {
            self.binding
        } else {
            (*self.state.adopted_binding.lock().ok()?)?
        };
        Some(ControlMessage::TriggerInputPad {
            id: self.id,
            start_s,
            end_s,
            exclusive,
            binding,
            received_at_ns,
            resident_control: Some(ResidentLaunchGuard {
                publication: self.publication.resident_launch_permit()?,
                cancelled: self.state.launch_cancelled.clone(),
            }),
            launch_revision,
        })
    }
}

pub(super) fn launch_with_producer(
    engine: &AudioEngine,
    ticket: &ResidentWindowTicket,
    exclusive: bool,
    received_at_ns: u64,
    producer: &Arc<Mutex<Producer<ControlMessage>>>,
) -> PyResult<bool> {
    if !Arc::ptr_eq(&ticket.ownership, &engine.input_runtime_ownership) {
        return Ok(false);
    }
    let mut producer = producer
        .lock()
        .map_err(|_| PyRuntimeError::new_err("producer lock poisoned"))?;
    if producer.is_full() {
        return Err(PyRuntimeError::new_err("resident command queue is full"));
    }
    let Some(message) = ticket.launch_message(exclusive, received_at_ns) else {
        return Ok(false);
    };
    engine
        .input_runtime_ownership
        .mark_launch_admitted(ticket.id);
    producer
        .push(message)
        .map_err(|_| PyRuntimeError::new_err("resident command queue is full"))?;
    Ok(true)
}

#[pymethods]
impl ResidentWindowTicket {
    /// Revoke this ticket's starts while preserving its acknowledged window.
    /// The flag belongs only to this ticket, so stale cancellation cannot affect a newer owner.
    pub fn cancel_launch(&self) -> bool {
        !self.state.launch_cancelled.swap(true, Ordering::AcqRel)
    }

    /// The adopted target comes from the actual voice's complete native extent.
    #[getter]
    pub fn effective_seek_seconds(&self) -> Option<f64> {
        (self.publication.status() == "accepted")
            .then(|| self.publication.resident_seek_seconds())
            .flatten()
    }

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

    /// An ACK alone cannot promote a ticket after unload, replacement or a newer intent.
    pub fn is_current(&self) -> bool {
        let binding =
            if self.publication.status() == "accepted" && !self.publication.preserved_window() {
                let Ok(adopted) = self.state.adopted_binding.lock() else {
                    return false;
                };
                let Some(binding) = *adopted else {
                    return false;
                };
                binding
            } else {
                self.binding
            };
        self.publication.current()
            && self.ownership.authority_current(self.id, binding)
            && self.ownership.binding_source_generation(self.id, binding)
                == Some(self.source_generation)
            && self.ownership.binding_window_current(self.id, binding)
            && self.acknowledgements.current_epoch(self.id)
                == binding
                    .accepted
                    .map_or(0, |accepted| accepted.publication_epoch)
            && self.publication.current()
    }

    /// A claimed callback tail cannot be cancelled or rolled back by metadata.
    pub fn cancel(&self) -> bool {
        let cancelled = self.publication.cancel_unclaimed();
        if cancelled {
            self.state.fail(
                &self.ownership,
                self.id,
                self.publication.expected,
                "resident preparation cancelled".into(),
                true,
            );
        }
        cancelled
    }
}

/// Called only after source/ticket validation and reserved producer capacity.
/// Register the pending generation and retire old resident intent before the
/// publication can be observed. Only fixed epoch guards enter queued launches.
pub(super) fn admit_stem_publication(
    engine: &AudioEngine,
    id: usize,
    set: &PreparedStemSet,
    source_version: &str,
    registration: Option<(PathBuf, PathBuf, Option<StemPairOwner>)>,
) -> PyResult<()> {
    let mut owners = engine
        .resident_stem_cache
        .lock()
        .map_err(|_| PyRuntimeError::new_err("resident stem ownership lock poisoned"))?;
    let owner = &mut owners[id];
    let next_intent =
        super::prepared_source::next_epoch(&owner.intent_epoch).map_err(PyRuntimeError::new_err)?;
    set.publication
        .mark_pending()
        .map_err(PyValueError::new_err)?;
    if let Some(pending) = &owner.relocation
        && pending.publication.cancel_unclaimed()
    {
        pending.state.fail(
            &engine.input_runtime_ownership,
            id,
            pending.publication.expected,
            "resident preparation superseded by stem publication".into(),
            true,
        );
    }
    owner.intent_epoch.store(next_intent, Ordering::Release);
    if let Some((cache_dir, generation_path, pair)) = registration {
        owner.pending = Some(StemOwner {
            set: set.clone(),
            source_version: source_version.to_owned(),
            cache_dir,
            generation_path,
            pair,
        });
    }
    Ok(())
}

pub(super) fn set_stem_pair_full_mix_with_producer(
    engine: &AudioEngine,
    id: usize,
    producer: &Arc<Mutex<Producer<ControlMessage>>>,
) -> PyResult<()> {
    if id >= NUM_SAMPLES {
        return Err(PyValueError::new_err("id out of range"));
    }
    let _requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    let mut producer = producer
        .lock()
        .map_err(|_| PyRuntimeError::new_err("control queue lock poisoned"))?;
    if producer.is_full() {
        return Err(PyRuntimeError::new_err("stem full-mix queue is full"));
    }
    let mut owners = engine
        .resident_stem_cache
        .lock()
        .map_err(|_| PyRuntimeError::new_err("resident stem ownership lock poisoned"))?;
    let owner = &mut owners[id];
    if let Some(previous) = owner.accepted.clone() {
        owner.remember(&previous)?;
    }
    let retired = [
        owner.accepted.as_ref().map(|value| value.set.clone()),
        owner.pending.as_ref().map(|value| value.set.clone()),
    ];
    producer
        .push(ControlMessage::SetStemPairFullMix { id, retired })
        .map_err(|_| PyRuntimeError::new_err("stem full-mix queue is full"))?;
    // The queued fixed handles, callback bank and actual jobs retain the PCM.
    // No potentially final large-buffer destruction occurs in this control path.
    owner.accepted = None;
    owner.pending = None;
    Ok(())
}

pub(super) fn reconcile(engine: &AudioEngine) -> PyResult<()> {
    let _requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    reconcile_under_request_lock(engine)
}

#[cfg(test)]
pub(super) fn history_count_for_test(engine: &AudioEngine, id: usize) -> usize {
    engine.resident_stem_cache.lock().unwrap()[id].history.len()
}

pub(super) fn cancel_all(engine: &AudioEngine) -> PyResult<()> {
    let owners = engine
        .resident_stem_cache
        .lock()
        .map_err(|_| PyRuntimeError::new_err("resident ownership lock poisoned"))?;
    for (id, owner) in owners.iter().enumerate() {
        if let Some(pending) = &owner.relocation
            && pending.publication.cancel_unclaimed()
        {
            pending.state.fail(
                &engine.input_runtime_ownership,
                id,
                pending.publication.expected,
                "resident preparation cancelled by shutdown".into(),
                true,
            );
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
        // Historical file leases are justified only by a real logical set
        // still held by a voice, queued transaction or job.
        owner
            .history
            .retain(|entry| entry.identity.strong_count() > 0);
        if let Some(pending) = owner.pending.as_ref() {
            match pending.set.publication.status() {
                "accepted" => {
                    if let Some(old) = owner.accepted.clone() {
                        owner.remember(&old)?;
                    }
                    owner.accepted = owner.pending.take();
                }
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
                pending.state.fail(
                    &engine.input_runtime_ownership,
                    id,
                    pending.publication.expected,
                    "resident native adoption deadline expired".into(),
                    true,
                );
            }
            let status = pending.publication.status();
            if status == "accepted" {
                engine
                    .input_runtime_ownership
                    .finish_resident_control(id, pending.publication.expected);
                let current = pending.sample.as_ref().is_some_and(|next| {
                    cache[id].as_ref().is_some_and(|sample| {
                        sample.same_source(next)
                            && sample.window_revision() == pending.expected_revision
                    })
                });
                let pending = owner.relocation.take().expect("checked pending window");
                if current && !pending.publication.preserved_window() {
                    cache[id] = pending.sample;
                    if let Some(accepted) = &mut owner.accepted
                        && let Some(stems) = pending.stems
                        && Arc::ptr_eq(
                            &accepted.set.complete_set_identity,
                            &stems.complete_set_identity,
                        )
                    {
                        accepted.set = stems;
                    }
                }
            } else if status == "rejected" {
                engine
                    .input_runtime_ownership
                    .finish_resident_control(id, pending.publication.expected);
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
            if let Some(old) = owner.accepted.clone() {
                owner.remember(&old)?;
            }
            owner.accepted = None;
        }
    }
    Ok(())
}

struct WindowWork {
    id: usize,
    old: SampleBuffer,
    bank: SampleBuffer,
    seek_receiver: Option<rtrb::Consumer<super::resident_seek::ResidentSeekPin>>,
    seek_pin: Option<super::resident_seek::ResidentSeekPin>,
    start: usize,
    end: usize,
    revision: u64,
    context: ResidentContext,
    intent: ResidentControlIntent,
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

#[cfg(test)]
fn admitted_window_peak(
    old: &SampleBuffer,
    start: usize,
    end: usize,
    stems: Option<&StemOwner>,
) -> Result<usize, String> {
    admitted_window_peak_with_readers(old, start, end, stems, &[])
}

fn admitted_window_peak_with_readers(
    old: &SampleBuffer,
    start: usize,
    end: usize,
    stems: Option<&StemOwner>,
    readers: &[Arc<[f32]>],
) -> Result<usize, String> {
    let frames = old.frame_count();
    let window_bytes = end
        .checked_sub(start)
        .filter(|frames| *frames > 0)
        .ok_or("invalid resident range")?
        .checked_mul(old.channels)
        .and_then(|n| n.checked_mul(4))
        .ok_or("resident PCM geometry overflow")?;
    if start >= end || end > frames {
        return Err("resident range exceeds complete source".into());
    }
    let mut backing = vec![old.samples.clone()];
    for samples in readers {
        if !backing.iter().any(|held| Arc::ptr_eq(held, samples)) {
            backing.push(samples.clone());
        }
    }
    if let Some(owner) = stems {
        for samples in std::iter::once(&owner.set.reference_samples)
            .chain(owner.set.stems.iter().map(|stem| &stem.samples))
        {
            if !backing.iter().any(|held| Arc::ptr_eq(held, samples)) {
                backing.push(samples.clone());
            }
        }
    }
    let held = backing.iter().try_fold(0_usize, |sum, samples| {
        samples
            .len()
            .checked_mul(4)
            .and_then(|bytes| sum.checked_add(bytes))
            .ok_or("resident held PCM overlap overflow")
    })?;
    let peak = if stems.is_some_and(|owner| owner.pair.is_none()) {
        // Legacy conversion still owns its old set while constructing complete
        // derivatives and the new crops. Those real old backings are additional
        // to the converter's reference/decode/alignment peak.
        super::stem_cache::admitted_stem_pcm_bytes(frames, old.channels)?
            .checked_add(held)
            .and_then(|bytes| {
                window_bytes
                    .checked_mul(5)
                    .and_then(|windows| bytes.checked_add(windows))
            })
            .and_then(|bytes| bytes.checked_add(64 * 1024))
            .ok_or("resident PCM overlap overflow")?
    } else {
        let source_new = if old.resident_start() == start && old.resident_end() == end {
            0
        } else {
            window_bytes
        };
        let stems_new = if let Some(owner) = stems {
            if owner.set.stems.iter().all(|stem| {
                stem.same_source(old)
                    && stem.resident_start() == start
                    && stem.resident_end() == end
            }) {
                0
            } else {
                window_bytes
                    .checked_mul(4)
                    .ok_or("resident component overlap overflow")?
            }
        } else {
            0
        };
        held.checked_add(source_new)
            .and_then(|bytes| bytes.checked_add(stems_new))
            .and_then(|bytes| {
                bytes.checked_add(if source_new != 0 || stems_new != 0 {
                    64 * 1024
                } else {
                    0
                })
            })
            .ok_or("resident PCM overlap overflow")?
    };
    if peak > super::cold_jobs::PCM_LIMIT_BYTES {
        return Err("resident preparation exceeds the 1-GiB transient PCM admission".into());
    }
    Ok(peak)
}

impl WindowWork {
    fn current(&self) -> bool {
        !self.cancelled.load(Ordering::Acquire)
            && self.publication.current()
            && self.binding.current()
    }

    fn prepare(&mut self) -> Result<(), String> {
        if let Some(mut receiver) = self.seek_receiver.take() {
            let deadline = Instant::now() + ADOPTION_DEADLINE;
            let pin = loop {
                if self.publication.status() == "accepted" {
                    return Ok(());
                }
                if !self.current() {
                    return Err("resident seek capture was cancelled or superseded".into());
                }
                if let Ok(pin) = receiver.pop() {
                    break pin;
                }
                if Instant::now() >= deadline {
                    return Err("resident seek capture deadline expired".into());
                }
                std::thread::sleep(Duration::from_millis(1));
            };
            if !pin.sample.same_source(&self.bank) {
                self.source_lease = self
                    .assets
                    .cold_lease_for_reader(&pin.sample)
                    .map_err(|error| error.to_string())?;
                self.old = pin.sample.clone();
                self.stem_owner = pin
                    .stems
                    .as_ref()
                    .map(|set| {
                        self.owners
                            .lock()
                            .map_err(|_| "resident ownership lock poisoned".to_string())?[self.id]
                            .owner_for(set)
                            .ok_or_else(|| {
                                "captured stem set has no retained immutable descriptor".to_string()
                            })
                    })
                    .transpose()?;
                self.seek_pin = Some(pin.clone());
                self.revision = self
                    .old
                    .window_revision()
                    .checked_add(1)
                    .ok_or("voice window revision exhausted")?;
            }
            self.start = 0;
            self.end = self.old.frame_count();
            self.context = if pin.key_lock {
                ResidentContext::KeyLockFullTrack
            } else {
                ResidentContext::FullTrack
            };
        }
        if !self.current() {
            return Err("resident source/window was superseded".into());
        }
        let held_readers = self
            .assets
            .held_reader_backings(
                &self.source_lease,
                self.stem_owner
                    .as_ref()
                    .map(|owner| owner.generation_path.as_path()),
            )
            .map_err(|error| error.to_string())?;
        let peak = admitted_window_peak_with_readers(
            &self.old,
            self.start,
            self.end,
            self.stem_owner.as_ref(),
            &held_readers,
        )?;
        #[cfg(test)]
        {
            super::cold_store::reset_window_read_observation_for_test();
            super::stem_pair::reset_range_observation_for_test();
        }
        let sample = self
            .source_lease
            .read_window_cancellable(
                &self.old,
                self.start,
                self.end,
                self.revision,
                self.context,
                super::cold_jobs::PCM_LIMIT_BYTES,
                &|| !self.current(),
            )
            .map_err(|error| error.to_string())?;
        if !self.current() {
            return Err("resident preparation cancelled after range read".into());
        }
        self.assets
            .retain_cold_reader(&self.source_lease, &sample)
            .map_err(|error| error.to_string())?;
        let stems = self
            .stem_owner
            .as_ref()
            .map(|owner| -> Result<PreparedStemSet, String> {
                // Only an unconverted legacy set still needs its complete WAV
                // converter. Accepted paired sets use their held PCM reader.
                let legacy_complete = if owner.pair.is_none() {
                    Some(
                        self.source_lease
                            .read_complete_cancellable(
                                &self.old,
                                super::cold_jobs::PCM_LIMIT_BYTES,
                                &|| !self.current(),
                            )
                            .map_err(|error| error.to_string())?,
                    )
                } else {
                    None
                };
                let mut set = owner.prepare_window(
                    legacy_complete.as_ref().unwrap_or(&sample),
                    &sample,
                    self.binding.binding.sample_rate_hz,
                    &self.assets,
                    &|| !self.current(),
                )?;
                set.complete_set_identity = owner.set.complete_set_identity.clone();
                set.accepted_timing = self
                    .seek_pin
                    .as_ref()
                    .map_or(self.binding.binding.accepted, |pin| pin.timing.accepted);
                set.publication = owner.set.publication.clone();
                self.assets
                    .retain_stems(owner.generation_path.clone(), &set)
                    .map_err(|error| error.to_string())?;
                Ok(set)
            })
            .transpose()?;
        #[cfg(test)]
        {
            *self.state.range_observation.lock().unwrap() = Some(WindowReadObservation {
                source: super::cold_store::window_read_observation_for_test(),
                stems: super::stem_pair::range_observation_for_test(),
                admitted_peak_bytes: peak,
            });
        }
        #[cfg(not(test))]
        let _ = peak;
        // Preparation owns the already-admitted payload while the realtime command
        // lane is briefly full. Retry is bounded, off-thread and cancellable.
        for attempt in 0..8 {
            if !self.current() {
                return Err(
                    "resident preparation superseded while waiting for command capacity".into(),
                );
            }
            if !self
                .producer
                .lock()
                .map_err(|_| "producer lock poisoned")?
                .is_full()
            {
                break;
            }
            if attempt == 7 {
                return Err("resident command queue is full".into());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let requests = self.requests.lock().map_err(|_| "request lock poisoned")?;
        let cache = self
            .cache
            .lock()
            .map_err(|_| "sample cache lock poisoned")?;
        if requests[self.id] != self.request
            || !self.current()
            || !cache[self.id]
                .as_ref()
                .is_some_and(|current| current.same_window(&self.bank))
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
        let mut adopted_binding = self.binding.binding;
        adopted_binding.resident = sample.resident_binding();
        *self
            .state
            .adopted_binding
            .lock()
            .map_err(|_| "resident binding lock poisoned")? = Some(adopted_binding);
        producer
            .push(ControlMessage::RelocateResident(Box::new(
                crate::messages::ResidentTransaction {
                    id: self.id,
                    sample,
                    stems,
                    binding: self.binding.binding,
                    publication: self.publication.clone(),
                    expected_window_revision: self.bank.window_revision(),
                    intent: self.intent,
                    seek_pin: self.seek_pin.clone(),
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
    prepare_window_with_producer(
        engine,
        id,
        WindowRequest {
            storage_range: Some((start_s, end_s)),
            ..WindowRequest::default()
        },
        producer,
    )
}

#[derive(Default, Clone, Copy)]
pub(super) struct WindowRequest {
    pub(super) storage_range: Option<(f64, f64)>,
    pub(super) loop_region: Option<(f64, Option<f64>)>,
    pub(super) seek_position_s: Option<f64>,
    pub(super) key_lock: Option<bool>,
}

pub(super) fn prepare_control(
    engine: &AudioEngine,
    id: usize,
    request: WindowRequest,
) -> PyResult<ResidentWindowTicket> {
    let handle = engine
        .stream_handle
        .as_ref()
        .ok_or_else(|| PyRuntimeError::new_err("Audio engine not initialized"))?;
    prepare_window_with_producer(engine, id, request, handle.producer.clone())
}

pub(super) fn prepare_window_with_producer(
    engine: &AudioEngine,
    id: usize,
    request: WindowRequest,
    producer: Arc<Mutex<Producer<ControlMessage>>>,
) -> PyResult<ResidentWindowTicket> {
    if id >= NUM_SAMPLES
        || request.storage_range.is_some_and(|(start, end)| {
            !start.is_finite() || !end.is_finite() || start < 0.0 || end <= start
        })
        || request.loop_region.is_some_and(|(start, end)| {
            !start.is_finite()
                || start < 0.0
                || end.is_some_and(|end| !end.is_finite() || end < 0.0)
        })
        || request
            .seek_position_s
            .is_some_and(|position| !position.is_finite() || position < 0.0)
    {
        return Err(PyValueError::new_err("invalid resident source range"));
    }
    reconcile(engine)?;
    let seek_only = request.seek_position_s.is_some()
        && request.loop_region.is_none()
        && request.key_lock.is_none()
        && request.storage_range.is_none();
    let binding = input_runtime_binding::capture(engine, id)?
        .filter(|current| (seek_only || current.available()) && current.current())
        .ok_or_else(|| PyValueError::new_err("current complete source/timing unavailable"))?;
    if producer
        .lock()
        .map_err(|_| PyRuntimeError::new_err("producer lock poisoned"))?
        .is_full()
    {
        return Err(PyRuntimeError::new_err("resident command queue is full"));
    }
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
    let rate = binding.binding.sample_rate_hz;
    let intent = ResidentControlIntent {
        loop_region: request.loop_region.map(|(start, end)| {
            let start = (start * f64::from(rate)).round() as usize;
            let end = end
                .map(|end| ((end * f64::from(rate)).round() as usize).max(start.saturating_add(1)));
            (start, end)
        }),
        seek_position_s: request.seek_position_s,
        key_lock: request.key_lock,
    };
    let key_lock = request.key_lock.unwrap_or(matches!(
        view.context,
        ResidentContext::KeyLockFullTrack | ResidentContext::KeyLockFiniteLoop
    ));
    let (start, end, context) = if request.seek_position_s.is_some() {
        (
            0,
            old.frame_count(),
            if key_lock {
                ResidentContext::KeyLockFullTrack
            } else {
                ResidentContext::FullTrack
            },
        )
    } else if let Some((start, end)) = intent.loop_region {
        let region = super::source_reader::effective_loop_region(start, end, old.frame_count())
            .ok_or_else(|| PyValueError::new_err("invalid complete source loop"))?;
        (
            region.start,
            region.end,
            if key_lock {
                ResidentContext::KeyLockFiniteLoop
            } else if region.start == 0 && region.end == old.frame_count() {
                ResidentContext::FullTrack
            } else {
                ResidentContext::FiniteLoop
            },
        )
    } else if let Some((start_s, end_s)) = request.storage_range {
        let start = (start_s * f64::from(rate)).round() as usize;
        let end = (end_s * f64::from(rate)).round() as usize;
        (
            start,
            end,
            if key_lock {
                ResidentContext::KeyLockFiniteLoop
            } else if start == 0 && end == old.frame_count() {
                ResidentContext::FullTrack
            } else {
                ResidentContext::FiniteLoop
            },
        )
    } else if key_lock && !view.context.permits_finite_range() {
        (0, old.frame_count(), ResidentContext::KeyLockFullTrack)
    } else {
        (old.resident_start(), old.resident_end(), view.context)
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
    // Admission and the immediate voice-capture command share exclusive producer
    // capacity before a previous intent can be superseded.
    let mut admission_producer = producer
        .lock()
        .map_err(|_| PyRuntimeError::new_err("producer lock poisoned"))?;
    if admission_producer.is_full() {
        return Err(PyRuntimeError::new_err("resident command queue is full"));
    }
    let mut owners = engine
        .resident_stem_cache
        .lock()
        .map_err(|_| PyRuntimeError::new_err("resident ownership lock poisoned"))?;
    if owners[id].pending.is_some() {
        return Err(PyRuntimeError::new_err(
            "resident stem publication is pending",
        ));
    }
    // Identical storage reuses already owned immutable PCM. It still queues the
    // existing native transaction and requires its real callback ACK before launch.
    // No seek capture or changed context can enter this worker-free branch.
    let reuse_window = request.seek_position_s.is_none()
        && start == old.resident_start()
        && end == old.resident_end()
        && context == view.context
        && binding.current()
        && old.resident_binding() == binding.binding.resident;
    let ready_stems = if reuse_window {
        owners[id]
            .accepted
            .as_ref()
            .map(|owner| {
                let mut set = owner.set.clone();
                // Accepted source PCM follows the actual acknowledged projection;
                // native timing adoption may have refreshed it since owner capture.
                set.accepted_timing = binding.binding.accepted;
                if !super::source_reader::prepared_stem_set_matches_sample(
                    &set,
                    &old,
                    old.channels,
                    rate as f32,
                    old.frame_count(),
                ) {
                    return Err(PyValueError::new_err(
                        "accepted resident stem/source window differs",
                    ));
                }
                Ok(set)
            })
            .transpose()?
    } else {
        None
    };
    // Required heavy preparation retains its original capacity-before-supersession
    // rule. A valid ready start never consumes or waits for a cold-lane slot.
    let reservation = if reuse_window {
        engine
            .cold_jobs
            .ensure_open()
            .map_err(PyRuntimeError::new_err)?;
        None
    } else {
        let reservation = engine
            .cold_jobs
            .reserve()
            .map_err(PyRuntimeError::new_err)?;
        if !seek_only {
            let held_readers = engine
                .project_assets
                .held_reader_backings(
                    &source_lease,
                    owners[id]
                        .accepted
                        .as_ref()
                        .map(|owner| owner.generation_path.as_path()),
                )
                .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
            admitted_window_peak_with_readers(
                &old,
                start,
                end,
                owners[id].accepted.as_ref(),
                &held_readers,
            )
            .map_err(PyRuntimeError::new_err)?;
        }
        Some(reservation)
    };
    let revision = if reuse_window {
        old.window_revision()
    } else {
        old.window_revision()
            .checked_add(1)
            .ok_or_else(|| PyValueError::new_err("window revision exhausted"))?
    };
    if owners[id]
        .relocation
        .as_ref()
        .is_some_and(|pending| pending.publication.status() == "adopting")
    {
        return Err(PyRuntimeError::new_err(
            "resident native adoption is in progress",
        ));
    }
    let next_intent = super::prepared_source::next_epoch(&owners[id].intent_epoch)
        .map_err(PyRuntimeError::new_err)?;
    if let Some(previous) = owners[id].relocation.as_ref() {
        if !previous.publication.cancel_unclaimed() {
            return Err(PyRuntimeError::new_err(
                "resident native adoption is in progress",
            ));
        }
        previous.state.fail(
            &engine.input_runtime_ownership,
            id,
            previous.publication.expected,
            "resident preparation superseded by newer intent".into(),
            true,
        );
    }
    owners[id]
        .intent_epoch
        .store(next_intent, Ordering::Release);
    engine
        .input_runtime_ownership
        .begin_resident_control(id, next_intent);
    let publication = PreparedSourcePermit::new(owners[id].intent_epoch.clone(), next_intent)
        .with_source_epoch(
            engine.prepared_source_epochs[id].clone(),
            engine.prepared_source_epochs[id].load(Ordering::Acquire),
        );
    let state = Arc::new(WindowState::default());
    let token = Arc::new(());
    let ticket = ResidentWindowTicket {
        publication: publication.clone(),
        state: state.clone(),
        window_revision: revision,
        previous_window_revision: old.window_revision(),
        start_frame: start,
        end_frame: end,
        full_frame_count: old.frame_count(),
        id,
        source_generation: binding.source_generation,
        binding: binding.binding,
        ownership: binding.ownership.clone(),
        acknowledgements: binding.acknowledgements.clone(),
        loop_region: request.loop_region,
    };
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
    if reuse_window {
        let pending = owners[id].relocation.as_mut().expect("new resident owner");
        pending.sample = Some(old.clone());
        pending.stems = ready_stems.clone();
        *state
            .adopted_binding
            .lock()
            .map_err(|_| PyRuntimeError::new_err("resident binding lock poisoned"))? =
            Some(binding.binding);
        publication
            .mark_pending()
            .map_err(PyRuntimeError::new_err)?;
        admission_producer
            .push(ControlMessage::RelocateResident(Box::new(
                crate::messages::ResidentTransaction {
                    id,
                    sample: old,
                    stems: ready_stems,
                    binding: binding.binding,
                    publication,
                    expected_window_revision: revision,
                    intent,
                    seek_pin: None,
                },
            )))
            .map_err(|_| PyRuntimeError::new_err("reserved single-producer capacity lost"))?;
        return Ok(ticket);
    }
    let (capture, seek_receiver) = if seek_only {
        let (sender, receiver) = rtrb::RingBuffer::new(1);
        (
            Some(Arc::new(super::resident_seek::ResidentSeekCapture::new(
                id,
                binding.binding,
                publication.clone(),
                request
                    .seek_position_s
                    .expect("validated seek-only request"),
                sender,
            ))),
            Some(receiver),
        )
    } else {
        (None, None)
    };
    let mut work = WindowWork {
        id,
        old: old.clone(),
        bank: old,
        seek_receiver,
        seek_pin: None,
        start,
        end,
        revision,
        context,
        intent,
        request: requests[id],
        binding,
        source_lease,
        stem_owner,
        requests: engine.pad_request_ids.clone(),
        cache: engine.sample_cache.clone(),
        owners: engine.resident_stem_cache.clone(),
        assets: engine.project_assets.clone(),
        producer: producer.clone(),
        publication: publication.clone(),
        state: state.clone(),
        token,
        cancelled: engine.cold_cancelled.clone(),
    };
    drop(owners);
    drop(cache);
    drop(requests);
    if let Some(capture) = capture
        && admission_producer
            .push(ControlMessage::CaptureResidentSeek(capture))
            .is_err()
    {
        publication.cancel_unclaimed();
        state.fail(
            &engine.input_runtime_ownership,
            id,
            publication.expected,
            "resident command queue is full".into(),
            false,
        );
        return Err(PyRuntimeError::new_err("resident command queue is full"));
    }
    drop(admission_producer);
    let mut guard = JobGuard {
        publication: publication.clone(),
        state: state.clone(),
        id,
        ownership: engine.input_runtime_ownership.clone(),
        enqueued: false,
    };
    engine
        .cold_jobs
        .submit(
            reservation.expect("new storage reserved cold capacity"),
            move || match work.prepare() {
                Ok(()) => guard.mark_enqueued(),
                Err(error) => {
                    let cancelled = !work.current();
                    work.publication.cancel_unclaimed();
                    work.state.fail(
                        &work.binding.ownership,
                        work.id,
                        work.publication.expected,
                        error,
                        cancelled,
                    );
                }
            },
        )
        .map_err(PyRuntimeError::new_err)?;
    Ok(ticket)
}

#[cfg(test)]
#[path = "resident_terminal_order_tests.rs"]
mod terminal_order_tests;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_peak_charges_distinct_live_old_component_backing_before_conversion() {
        // This is an admission-only geometry fixture, not a source/ACK oracle.
        // Tiny real arrays expose a boundary without allocating complete PCM.
        let held = 64 * 4;
        let crop = 4;
        let fixed = 4 * 1024 * 1024 + 64 * 1024 + crop * 5 + held;
        let frames = (super::super::cold_jobs::PCM_LIMIT_BYTES - fixed) / 40;
        let old = SampleBuffer {
            channels: 1,
            samples: Arc::from([0.0; 64]),
            residency: Some(Arc::new(crate::messages::ResidentSourceView {
                source: Arc::new(crate::messages::CompleteSourceIdentity {
                    frame_count: frames,
                    channels: 1,
                    sample_rate_hz: 48_000,
                    original_sha256: [0; 32],
                    playback_sha256: [0; 32],
                    mono_sha256: [0; 32],
                    transform_sha256: [0; 32],
                    source_zero_frame: 0,
                }),
                start_frame: 0,
                window_revision: 1,
                context: ResidentContext::FiniteLoop,
            })),
        };
        let mut owner = StemOwner {
            set: PreparedStemSet {
                complete_set_identity: Arc::new([0; 32]),
                accepted_timing: None,
                reference_samples: old.samples.clone(),
                publication: PreparedSourcePermit::unbound(),
                source_version_hash: 0,
                sample_rate_hz: 48_000,
                channels: 1,
                frame_count: frames,
                available_mask: 0b1111,
                stems: std::array::from_fn(|_| old.clone()),
            },
            source_version: "admission-only".into(),
            cache_dir: PathBuf::new(),
            generation_path: PathBuf::new(),
            pair: None,
        };
        let shared = admitted_window_peak(&old, 1, 2, Some(&owner)).unwrap();
        for stem in &mut owner.set.stems {
            stem.samples = Arc::from([0.0; 64]);
        }
        assert!(admitted_window_peak(&old, 1, 2, Some(&owner)).is_err());
        let smaller = admitted_window_peak(&old, 1, 2, None).unwrap();
        assert!(smaller < shared);
        assert_eq!(old.samples.len(), 64);
    }
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
        let ownership = Arc::new(InputRuntimeOwnership::default());
        ownership.begin_resident_control(0, publication.expected);
        let mut guard = JobGuard {
            publication: publication.clone(),
            state: state.clone(),
            id: 0,
            ownership: ownership.clone(),
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
        assert!(!ownership.resident_control_pending(0));
        *gate.0.lock().unwrap() = true;
        gate.1.notify_all();
        shutdown.join().unwrap();
    }

    #[test]
    fn actual_cold_worker_panic_finishes_only_its_exact_pending_input_owner() {
        for newer_intent in [false, true] {
            let mut lane = super::super::cold_jobs::ColdJobs::new().unwrap();
            let publication = PreparedSourcePermit::unrestricted();
            let state = Arc::new(WindowState::default());
            let ownership = Arc::new(InputRuntimeOwnership::default());
            ownership.begin_resident_control(0, publication.expected);
            let guard = JobGuard {
                publication: publication.clone(),
                state: state.clone(),
                id: 0,
                ownership: ownership.clone(),
                enqueued: false,
            };
            if newer_intent {
                ownership.begin_resident_control(0, publication.expected + 1);
            }
            lane.submit(lane.reserve().unwrap(), move || {
                let _guard = guard;
                panic!("injected resident preparation failure");
            })
            .unwrap();
            let deadline = Instant::now() + Duration::from_secs(3);
            while state.terminal.load(Ordering::Acquire) == 0 && Instant::now() < deadline {
                std::thread::yield_now();
            }
            assert_eq!(state.terminal.load(Ordering::Acquire), 2);
            lane.shutdown();
            assert_eq!(publication.status(), "rejected");
            assert_eq!(
                ownership.resident_control_pending(0),
                newer_intent,
                "failed old job cleared a newer native readiness owner"
            );
        }
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
