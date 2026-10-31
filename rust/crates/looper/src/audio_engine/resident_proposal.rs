//! Bounded preparation proposals and explicit resident allocation context.
//! Proposal admission never advances current source or live control authority.
use super::content_placement::ContentCell;
use super::project_assets::ProjectAssets;
use super::resident_accounting::{ResidentAccount, ResidentAdmissionError, ResidentReservation};
use crate::messages::SampleBuffer;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

/// Serialize preparation peaks and immutable reuse, independently of control admission.
/// Never enter this gate while holding requests, owners, the producer or Python.
pub(super) fn lock_preparation(
    gate: &Mutex<()>,
    current: impl Fn() -> bool,
) -> Result<std::sync::MutexGuard<'_, ()>, String> {
    loop {
        if !current() {
            return Err("resident preparation superseded while waiting".into());
        }
        match gate.try_lock() {
            Ok(guard) => return Ok(guard),
            Err(std::sync::TryLockError::Poisoned(_)) => {
                return Err("resident preparation lock poisoned".into());
            }
            Err(std::sync::TryLockError::WouldBlock) => {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }
    }
}

pub(super) struct ResidentProposals {
    epochs: Vec<[Arc<AtomicU64>; 4]>,
    stem_admission_orders: Vec<[Arc<ResidentAdmissionClock>; 2]>,
    sources: Vec<Mutex<Option<SourceEventFence>>>,
}
struct SourceEventFence {
    proposal: ResidentProposal,
    request: u64,
    base_request: u64,
    base_epoch: u64,
}

impl Default for ResidentProposals {
    fn default() -> Self {
        Self {
            epochs: (0..super::constants::NUM_SAMPLES)
                .map(|_| std::array::from_fn(|_| Arc::new(AtomicU64::new(0))))
                .collect(),
            stem_admission_orders: (0..super::constants::NUM_SAMPLES)
                .map(|_| std::array::from_fn(|_| Arc::default()))
                .collect(),
            sources: (0..super::constants::NUM_SAMPLES)
                .map(|_| Mutex::new(None))
                .collect(),
        }
    }
}

#[derive(Default)]
struct ResidentAdmissionClock {
    next: AtomicU64,
    admitted: AtomicU64,
}

/// Capturing never revokes another context or an admitted proposal/reader.
pub(super) struct ResidentAdmissionOrder {
    clock: Arc<ResidentAdmissionClock>,
    expected: u64,
}
impl ResidentAdmissionOrder {
    pub(super) fn current(&self) -> bool {
        self.clock.admitted.load(Ordering::Acquire) < self.expected
    }
    /// Caller holds requests through the successful reserved capture enqueue.
    pub(super) fn mark_admitted(&self) {
        debug_assert!(self.current());
        self.clock.admitted.store(self.expected, Ordering::Release);
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ResidentProposal {
    epoch: Arc<AtomicU64>,
    expected: u64,
    cancelled: Arc<AtomicBool>,
    armed: Arc<AtomicBool>,
    source_epoch: Option<(Arc<AtomicU64>, u64)>,
}

impl ResidentProposals {
    /// The caller serializes successful preflight and admission with pad requests.
    /// Explicit/candidate and policy domains retain their independent live interests.
    pub(super) fn capture_stem_admission_order(
        &self,
        cell: ContentCell,
        policy: bool,
    ) -> Result<ResidentAdmissionOrder, String> {
        let clock = &self
            .stem_admission_orders
            .get(cell.index())
            .ok_or("resident admission content cell out of range")?[usize::from(policy)];
        let expected = clock
            .next
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                value.checked_add(1)
            })
            .map_err(|_| "resident admission order exhausted")?
            + 1;
        Ok(ResidentAdmissionOrder {
            clock: clock.clone(),
            expected,
        })
    }
    /// Preparation domains belong to stable content cells. Physical loader request
    /// filters below remain at their original slots and are never permuted.
    pub(super) fn capture_pair_fences_for_cell(
        &self,
        cell: ContentCell,
    ) -> Option<[(Arc<AtomicU64>, u64); 4]> {
        self.capture_pair_fences(cell.index())
    }
    pub(super) fn begin_for_cell(&self, cell: ContentCell) -> Result<ResidentProposal, String> {
        self.begin_domain(cell.index(), 1)
    }
    pub(super) fn begin_stems_for_cell(
        &self,
        cell: ContentCell,
    ) -> Result<ResidentProposal, String> {
        self.begin_domain(cell.index(), 2)
    }
    pub(super) fn begin_policy_for_cell(
        &self,
        cell: ContentCell,
    ) -> Result<ResidentProposal, String> {
        self.begin_domain(cell.index(), 3)
    }
    pub(super) fn begin_source_for_cell(
        &self,
        id: usize,
        cell: ContentCell,
        base_request: u64,
        base_epoch: u64,
    ) -> Result<(ResidentProposal, u64), String> {
        self.begin_source_in_cell(id, cell.index(), base_request, base_epoch)
    }
    pub(super) fn capture_pair_fences(&self, id: usize) -> Option<[(Arc<AtomicU64>, u64); 4]> {
        Some(self.epochs.get(id)?.clone().map(|epoch| {
            let expected = epoch.load(Ordering::Acquire);
            (epoch, expected)
        }))
    }
    #[cfg(test)]
    pub(super) fn begin(&self, id: usize) -> Result<ResidentProposal, String> {
        self.begin_domain(id, 1)
    }
    #[cfg(test)]
    pub(super) fn begin_source(
        &self,
        id: usize,
        base_request: u64,
        base_epoch: u64,
    ) -> Result<(ResidentProposal, u64), String> {
        self.begin_source_in_cell(id, id, base_request, base_epoch)
    }
    fn begin_source_in_cell(
        &self,
        id: usize,
        cell: usize,
        base_request: u64,
        base_epoch: u64,
    ) -> Result<(ResidentProposal, u64), String> {
        let mut source = self
            .sources
            .get(id)
            .ok_or("resident source proposal pad out of range")?
            .lock()
            .map_err(|_| "resident source proposal lock poisoned")?;
        let request = source
            .as_ref()
            .map_or(base_request, |source| source.request.max(base_request))
            .checked_add(1)
            .filter(|request| *request <= u64::MAX / 4)
            .ok_or("cold acknowledgement generation exhausted")?;
        let proposal = self.begin_domain(cell, 0)?;
        *source = Some(SourceEventFence {
            proposal: proposal.clone(),
            request,
            base_request,
            base_epoch,
        });
        Ok((proposal, request))
    }
    pub(super) fn source_event_matches(
        &self,
        id: usize,
        request: u64,
        current_request: u64,
        current_epoch: u64,
    ) -> Option<bool> {
        let source = self.sources.get(id)?.lock().ok()?;
        let source = source.as_ref()?;
        Some(
            source.request == request
                && source.proposal.current()
                && if source.proposal.armed() {
                    current_request == request
                } else {
                    current_request == source.base_request && current_epoch == source.base_epoch
                },
        )
    }
    pub(super) fn source_error_matches(&self, id: usize, request: u64) -> Option<bool> {
        let source = self.sources.get(id)?.lock().ok()?;
        let source = source.as_ref()?;
        Some(source.request == request && source.proposal.current())
    }
    #[cfg(test)]
    pub(super) fn begin_stems(&self, id: usize) -> Result<ResidentProposal, String> {
        self.begin_domain(id, 2)
    }
    #[cfg(test)]
    pub(super) fn begin_policy(&self, id: usize) -> Result<ResidentProposal, String> {
        self.begin_domain(id, 3)
    }
    fn begin_domain(&self, id: usize, domain: usize) -> Result<ResidentProposal, String> {
        let epoch = &self
            .epochs
            .get(id)
            .ok_or("resident proposal pad out of range")?[domain];
        let expected = epoch
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                value.checked_add(1)
            })
            .map_err(|_| "resident proposal generation exhausted")?
            + 1;
        Ok(ResidentProposal {
            epoch: epoch.clone(),
            expected,
            cancelled: Arc::new(AtomicBool::new(false)),
            armed: Arc::new(AtomicBool::new(false)),
            source_epoch: None,
        })
    }
}

impl ResidentProposal {
    pub(crate) fn current(&self) -> bool {
        !self.cancelled.load(Ordering::Acquire)
            && self.epoch.load(Ordering::Acquire) == self.expected
            && self
                .source_epoch
                .as_ref()
                .is_none_or(|(epoch, expected)| epoch.load(Ordering::Acquire) == *expected)
    }
    pub(super) fn with_source_epoch(mut self, epoch: Arc<AtomicU64>, expected: u64) -> Self {
        self.source_epoch = Some((epoch, expected));
        self
    }
    pub(super) fn armed(&self) -> bool {
        self.armed.load(Ordering::Acquire)
    }
    pub(super) fn arm(&self) -> Result<(), String> {
        if !self.try_arm() {
            return Err("resident proposal superseded before arming".into());
        }
        Ok(())
    }
    pub(super) fn try_arm(&self) -> bool {
        if !self.current() {
            return false;
        }
        self.armed.store(true, Ordering::Release);
        true
    }
    pub(super) fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
}

/// Only final selected live views use this context. Complete timing, analysis,
/// conversion and export reads deliberately do not receive one.
#[derive(Clone)]
pub(super) struct ResidentReadContext {
    pub(super) assets: Arc<ProjectAssets>,
    pub(super) account: Arc<ResidentAccount>,
    registrations: Arc<Mutex<Vec<ResidentReservation>>>,
    assignment: bool,
    history: Option<(Arc<super::project_assets::PcmHistory>, usize)>,
}

impl ResidentReadContext {
    pub(super) fn new(assets: Arc<ProjectAssets>, account: Arc<ResidentAccount>) -> Self {
        Self {
            assets,
            account,
            registrations: Arc::new(Mutex::new(Vec::new())),
            assignment: true,
            history: None,
        }
    }
    pub(super) fn reader(assets: Arc<ProjectAssets>, account: Arc<ResidentAccount>) -> Self {
        Self {
            assets,
            account,
            registrations: Arc::new(Mutex::new(Vec::new())),
            assignment: false,
            history: None,
        }
    }
    pub(super) fn with_history(
        mut self,
        history: Arc<super::project_assets::PcmHistory>,
        id: usize,
    ) -> Self {
        self.history = Some((history, id));
        self
    }
    pub(super) fn reader_slots(&self) -> usize {
        usize::from(self.assignment)
    }
    pub(super) fn reserve(
        &self,
        bytes: usize,
        reused: &[&SampleBuffer],
        owners: usize,
    ) -> Result<ResidentReservation, ResidentAdmissionError> {
        let mut token = self
            .assets
            .reserve_resident(&self.account, bytes, reused, owners)?;
        if let Some((history, id)) = self.history.as_ref() {
            token.reserve_history_slot(history, *id, reused.first().copied())?;
        }
        Ok(token)
    }
    pub(super) fn commit_sample(
        &self,
        token: &mut ResidentReservation,
        sample: &SampleBuffer,
    ) -> Result<(), ResidentAdmissionError> {
        if self.history.is_some() && !self.assignment {
            token.commit_source_history(sample)
        } else {
            token.commit(&[sample])
        }
    }
    pub(super) fn commit_selected_history(&self, sample: &SampleBuffer) -> Result<(), String> {
        let mut token = self.take_registration()?;
        token
            .commit_source_history(sample)
            .map_err(|error| error.to_string())?;
        self.retain_registration(token)
    }
    pub(super) fn retain_registration(
        &self,
        reservation: ResidentReservation,
    ) -> Result<(), String> {
        if !self.assignment {
            return Ok(());
        }
        self.registrations
            .lock()
            .map_err(|_| "resident registration lock poisoned")?
            .push(reservation);
        Ok(())
    }
    pub(super) fn take_registration(&self) -> Result<ResidentReservation, String> {
        self.registrations
            .lock()
            .map_err(|_| "resident registration lock poisoned")?
            .pop()
            .ok_or_else(|| "resident reader capacity was not reserved before allocation".into())
    }
    pub(super) fn copy_selected(&self, sample: &SampleBuffer) -> Result<SampleBuffer, String> {
        let bytes = sample
            .samples
            .len()
            .checked_mul(4)
            .ok_or("resident copy extent overflow")?;
        let mut reservation = self
            .reserve(bytes, &[], 1)
            .map_err(|error| error.to_string())?;
        reservation
            .reserve_reader_slots(1)
            .map_err(|error| error.to_string())?;
        let mut copied = sample.clone();
        copied.samples = Arc::from(sample.samples.as_ref());
        copied.residency = sample
            .residency
            .as_ref()
            .map(|view| Arc::new((**view).clone()));
        self.commit_sample(&mut reservation, &copied)
            .map_err(|error| error.to_string())?;
        self.retain_registration(reservation)?;
        Ok(copied)
    }
}

pub(super) fn admission_io(error: ResidentAdmissionError) -> std::io::Error {
    error.into_io_error()
}

pub(super) fn is_admission_io(error: &std::io::Error) -> bool {
    error
        .get_ref()
        .is_some_and(|inner| inner.is::<ResidentAdmissionError>())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn proposals_supersede_without_touching_current_authority() {
        let proposals = ResidentProposals::default();
        let current = AtomicU64::new(7);
        let first = proposals.begin(0).unwrap();
        let second = proposals.begin(0).unwrap();
        assert!(!first.current());
        assert!(first.arm().is_err());
        assert_eq!(current.load(Ordering::Acquire), 7);
        second.arm().unwrap();
        assert!(second.armed());
        second.cancel();
        assert!(!second.current());
        assert_eq!(current.load(Ordering::Acquire), 7);
    }
    #[test]
    fn failed_source_proposals_keep_current_authority_and_unique_visible_requests() {
        let proposals = ResidentProposals::default();
        let (first, first_id) = proposals.begin_source(0, 7, 9).unwrap();
        assert_eq!(first_id, 8);
        assert_eq!(proposals.source_event_matches(0, 8, 7, 9), Some(true));
        let (_, second_id) = proposals.begin_source(0, 7, 9).unwrap();
        assert_eq!(second_id, 9);
        assert!(!first.current());
        assert_eq!(proposals.source_event_matches(0, 8, 7, 9), Some(false));
        assert_eq!(proposals.source_event_matches(0, 9, 7, 9), Some(true));
        assert_eq!(proposals.source_event_matches(0, 9, 8, 10), Some(false));
    }
    #[test]
    fn policy_proposals_never_supersede_explicit_stem_preparation() {
        let proposals = ResidentProposals::default();
        let explicit = proposals.begin_stems(0).unwrap();
        let first_policy = proposals.begin_policy(0).unwrap();
        let second_policy = proposals.begin_policy(0).unwrap();
        assert!(explicit.current());
        assert!(!first_policy.current());
        assert!(second_policy.current());
        let newer_explicit = proposals.begin_stems(0).unwrap();
        assert!(!explicit.current());
        assert!(newer_explicit.current());
        assert!(second_policy.current());
    }
}
