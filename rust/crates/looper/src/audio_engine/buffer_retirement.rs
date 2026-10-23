use super::constant_timing::PreparedConstantTiming;
use crate::messages::{PreparedStemSet, SampleBuffer};
use rtrb::{Consumer, Producer, PushError, RingBuffer};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub(crate) const RETIRED_AUDIO_BUFFER_QUEUE_CAPACITY: usize = 1024;
const RETIRED_AUDIO_BUFFER_BACKLOG_CAPACITY: usize = 128;

// The inner fields are never pattern-matched: the consumer thread simply drops
// the enum to release Arc-backed audio memory off the realtime callback thread.
// Clippy flags the fields as "never read", but they exist solely to transfer
// ownership across the thread boundary.
#[allow(dead_code)]
pub(crate) enum RetiredAudioBuffer {
    MixCapture(Box<super::mix_capture::CaptureRt>),
    CapturePublication(Arc<super::mix_capture::publication::CapturePublication>),
    Sample(SampleBuffer),
    ColdAdoption(Arc<std::sync::atomic::AtomicU8>),
    PreparedStems(PreparedStemSet),
    ConstantTiming(PreparedConstantTiming),
    GlobalPlaybackBatch(Arc<super::global_playback_batch::GlobalPlaybackBatch>),
    AcceptedTimingRefresh(Arc<super::accepted_timing_refresh::AcceptedTimingRefresh>),
    RuntimePolicy(super::input_mapping::RuntimePolicyWork),
    ResidentTransaction(Box<crate::messages::ResidentTransaction>),
    ResidentCancellation(Arc<AtomicBool>),
    ResidentSeekCapture(Arc<super::resident_seek::ResidentSeekCapture>),
    CommittedStemWork(Box<super::committed_stem_residency::CommittedStemRtWork>),
    PadCopy(Box<super::pad_copy::PadCopyRtWork>),
    BankMutation(Box<super::pad_copy::bank::BankMutationRtWork>),
    BankMutationPublication(Arc<super::pad_copy::bank::BankMutationPublication>),
    RecoveredBankProbe(Box<super::pad_copy::bank::recovery::RecoveredBankProbe>),
    PadPair(Box<super::pad_pair::PadPairRtWork>),
    HoldAction(Box<super::hold_action::HoldActionRtWork>),
    PitchAction(Box<super::pitch_actions::PitchActionWork>),
    PitchPublication(Arc<super::pitch_publication::PitchActionPublication>),
    OrdinaryStart(Box<super::ordinary_start::OrdinaryStartWork>),
    OrdinaryStartGroup(super::ordinary_start::OrdinaryStartGroup),
    OrdinaryStartPublication(Arc<super::ordinary_start::OrdinaryStartPublication>),
    PadStartTarget(super::pad_start::PadStartCustody),
    GlobalStartTarget(super::captured_global_start::CapturedGlobalStart),
    SourcePublication(super::input_runtime_binding::SourcePublication),
}

pub(crate) trait AudioBufferRetirement {
    #[cfg(test)]
    fn retire_runtime_policy(&mut self, work: super::input_mapping::RuntimePolicyWork) {
        drop(work);
    }
    #[cfg(not(test))]
    fn retire_runtime_policy(&mut self, work: super::input_mapping::RuntimePolicyWork);
    #[cfg(test)]
    fn retire_mix_capture(&mut self, work: Box<super::mix_capture::CaptureRt>) {
        drop(work);
    }
    #[cfg(not(test))]
    fn retire_mix_capture(&mut self, work: Box<super::mix_capture::CaptureRt>);
    #[cfg(test)]
    fn retire_capture_publication(
        &mut self,
        publication: Arc<super::mix_capture::publication::CapturePublication>,
    ) {
        drop(publication);
    }
    #[cfg(not(test))]
    fn retire_capture_publication(
        &mut self,
        publication: Arc<super::mix_capture::publication::CapturePublication>,
    );
    #[cfg(test)]
    fn retire_recovered_bank_probe(
        &mut self,
        work: Box<super::pad_copy::bank::recovery::RecoveredBankProbe>,
    ) {
        drop(work);
    }
    #[cfg(not(test))]
    fn retire_recovered_bank_probe(
        &mut self,
        work: Box<super::pad_copy::bank::recovery::RecoveredBankProbe>,
    );
    #[cfg(test)]
    fn retire_global_start_target(
        &mut self,
        owner: super::captured_global_start::CapturedGlobalStart,
    ) {
        drop(owner);
    }
    #[cfg(not(test))]
    fn retire_global_start_target(
        &mut self,
        owner: super::captured_global_start::CapturedGlobalStart,
    );
    #[cfg(test)]
    fn retire_ordinary_start(&mut self, work: Box<super::ordinary_start::OrdinaryStartWork>) {
        drop(work);
    }
    #[cfg(not(test))]
    fn retire_ordinary_start(&mut self, work: Box<super::ordinary_start::OrdinaryStartWork>);
    #[cfg(test)]
    fn retire_ordinary_start_group(&mut self, group: super::ordinary_start::OrdinaryStartGroup) {
        drop(group);
    }
    #[cfg(not(test))]
    fn retire_ordinary_start_group(&mut self, group: super::ordinary_start::OrdinaryStartGroup);
    #[cfg(test)]
    fn retire_ordinary_start_publication(
        &mut self,
        work: Arc<super::ordinary_start::OrdinaryStartPublication>,
    ) {
        drop(work);
    }
    #[cfg(not(test))]
    fn retire_ordinary_start_publication(
        &mut self,
        work: Arc<super::ordinary_start::OrdinaryStartPublication>,
    );
    #[cfg(test)]
    fn retire_pad_start_target(&mut self, owner: super::pad_start::PadStartCustody) {
        drop(owner);
    }
    #[cfg(not(test))]
    fn retire_pad_start_target(&mut self, owner: super::pad_start::PadStartCustody);
    #[cfg(test)]
    fn retire_pitch_action(&mut self, work: Box<super::pitch_actions::PitchActionWork>) {
        drop(work);
    }
    #[cfg(not(test))]
    fn retire_pitch_action(&mut self, work: Box<super::pitch_actions::PitchActionWork>);
    #[cfg(test)]
    fn retire_pitch_publication(
        &mut self,
        work: Arc<super::pitch_publication::PitchActionPublication>,
    ) {
        drop(work);
    }
    #[cfg(not(test))]
    fn retire_pitch_publication(
        &mut self,
        work: Arc<super::pitch_publication::PitchActionPublication>,
    );
    #[cfg(test)]
    fn retire_pad_pair(&mut self, work: Box<super::pad_pair::PadPairRtWork>) {
        drop(work);
    }
    #[cfg(not(test))]
    fn retire_pad_pair(&mut self, work: Box<super::pad_pair::PadPairRtWork>);
    #[cfg(test)]
    fn retire_hold_action(&mut self, work: Box<super::hold_action::HoldActionRtWork>) {
        drop(work);
    }
    #[cfg(not(test))]
    fn retire_hold_action(&mut self, work: Box<super::hold_action::HoldActionRtWork>);
    #[cfg(test)]
    fn retire_source_publication(
        &mut self,
        publication: super::input_runtime_binding::SourcePublication,
    ) {
        drop(publication);
    }
    #[cfg(not(test))]
    fn retire_source_publication(
        &mut self,
        publication: super::input_runtime_binding::SourcePublication,
    );
    #[cfg(test)]
    fn retire_bank_mutation(&mut self, work: Box<super::pad_copy::bank::BankMutationRtWork>) {
        drop(work);
    }
    #[cfg(not(test))]
    fn retire_bank_mutation(&mut self, work: Box<super::pad_copy::bank::BankMutationRtWork>);
    #[cfg(test)]
    fn retire_bank_publication(
        &mut self,
        work: Arc<super::pad_copy::bank::BankMutationPublication>,
    ) {
        drop(work);
    }
    #[cfg(not(test))]
    fn retire_bank_publication(
        &mut self,
        work: Arc<super::pad_copy::bank::BankMutationPublication>,
    );
    #[cfg(test)]
    fn retire_pad_copy(&mut self, work: Box<super::pad_copy::PadCopyRtWork>) {
        drop(work);
    }
    #[cfg(not(test))]
    fn retire_pad_copy(&mut self, work: Box<super::pad_copy::PadCopyRtWork>);
    #[cfg(test)]
    fn retire_committed_stem_work(
        &mut self,
        work: Box<super::committed_stem_residency::CommittedStemRtWork>,
    ) {
        // Existing test-only sinks may destroy immediately; the production sink overrides
        // this method and transfers the entire owner to its existing retirement worker.
        drop(work);
    }
    #[cfg(not(test))]
    fn retire_committed_stem_work(
        &mut self,
        work: Box<super::committed_stem_residency::CommittedStemRtWork>,
    );
    fn retire_resident_capture(&mut self, capture: Arc<super::resident_seek::ResidentSeekCapture>);
    fn retire_resident_cancellation(&mut self, cancelled: Arc<AtomicBool>);
    fn retire_resident_transaction(
        &mut self,
        publication: Box<crate::messages::ResidentTransaction>,
    );
    fn retire_cold_adoption(&mut self, adoption: Arc<std::sync::atomic::AtomicU8>);
    fn retire_sample(&mut self, sample: SampleBuffer);
    fn retire_prepared_stems(&mut self, stems: PreparedStemSet);
    fn retire_constant_timing(&mut self, timing: PreparedConstantTiming);
    fn retire_global_playback_batch(
        &mut self,
        batch: Arc<super::global_playback_batch::GlobalPlaybackBatch>,
    );
    fn retire_accepted_timing_refresh(
        &mut self,
        refresh: Arc<super::accepted_timing_refresh::AcceptedTimingRefresh>,
    );
    fn available_retirement_slots(&mut self) -> usize;
}

#[cfg(test)]
pub(crate) struct ImmediateAudioBufferRetirement;

#[cfg(test)]
impl AudioBufferRetirement for ImmediateAudioBufferRetirement {
    fn retire_committed_stem_work(
        &mut self,
        _: Box<super::committed_stem_residency::CommittedStemRtWork>,
    ) {
    }
    fn retire_resident_capture(&mut self, _: Arc<super::resident_seek::ResidentSeekCapture>) {}
    fn retire_resident_cancellation(&mut self, _: Arc<AtomicBool>) {}
    fn retire_resident_transaction(&mut self, _: Box<crate::messages::ResidentTransaction>) {}
    fn retire_cold_adoption(&mut self, _: Arc<std::sync::atomic::AtomicU8>) {}
    fn retire_accepted_timing_refresh(
        &mut self,
        _: Arc<super::accepted_timing_refresh::AcceptedTimingRefresh>,
    ) {
    }
    fn retire_sample(&mut self, _sample: SampleBuffer) {}

    fn retire_prepared_stems(&mut self, _stems: PreparedStemSet) {}
    fn retire_constant_timing(&mut self, _timing: PreparedConstantTiming) {}
    fn retire_global_playback_batch(
        &mut self,
        _batch: Arc<super::global_playback_batch::GlobalPlaybackBatch>,
    ) {
    }

    fn available_retirement_slots(&mut self) -> usize {
        usize::MAX
    }
}

pub(crate) struct RtAudioBufferRetirement {
    producer: Producer<RetiredAudioBuffer>,
    backlog: Box<[Option<RetiredAudioBuffer>]>,
    backlog_len: usize,
}

impl RtAudioBufferRetirement {
    fn new(producer: Producer<RetiredAudioBuffer>) -> Self {
        Self {
            producer,
            backlog: (0..RETIRED_AUDIO_BUFFER_BACKLOG_CAPACITY)
                .map(|_| None)
                .collect(),
            backlog_len: 0,
        }
    }

    fn retire_buffer(&mut self, buffer: RetiredAudioBuffer) {
        self.flush_backlog();

        match self.producer.push(buffer) {
            Ok(()) => {}
            Err(PushError::Full(buffer)) => {
                self.store_or_forget(buffer);
            }
        }
    }

    fn flush_backlog(&mut self) {
        if self.backlog_len == 0 {
            return;
        }

        for slot in self.backlog.iter_mut() {
            let Some(buffer) = slot.take() else {
                continue;
            };

            match self.producer.push(buffer) {
                Ok(()) => {
                    self.backlog_len -= 1;
                }
                Err(PushError::Full(buffer)) => {
                    *slot = Some(buffer);
                    return;
                }
            }
        }
    }

    fn store_or_forget(&mut self, buffer: RetiredAudioBuffer) {
        for slot in self.backlog.iter_mut() {
            if slot.is_none() {
                *slot = Some(buffer);
                self.backlog_len += 1;
                return;
            }
        }

        // Avoid a large final Arc drop on the callback thread even if the retirement path is
        // saturated. The queue/backlog are sized to make this a last-resort failure mode.
        std::mem::forget(buffer);
    }
}

impl AudioBufferRetirement for RtAudioBufferRetirement {
    fn retire_runtime_policy(&mut self, work: super::input_mapping::RuntimePolicyWork) {
        self.retire_buffer(RetiredAudioBuffer::RuntimePolicy(work));
    }
    fn retire_mix_capture(&mut self, work: Box<super::mix_capture::CaptureRt>) {
        self.retire_buffer(RetiredAudioBuffer::MixCapture(work));
    }
    fn retire_capture_publication(
        &mut self,
        publication: Arc<super::mix_capture::publication::CapturePublication>,
    ) {
        self.retire_buffer(RetiredAudioBuffer::CapturePublication(publication));
    }
    fn retire_recovered_bank_probe(
        &mut self,
        work: Box<super::pad_copy::bank::recovery::RecoveredBankProbe>,
    ) {
        self.retire_buffer(RetiredAudioBuffer::RecoveredBankProbe(work));
    }
    fn retire_global_start_target(
        &mut self,
        owner: super::captured_global_start::CapturedGlobalStart,
    ) {
        self.retire_buffer(RetiredAudioBuffer::GlobalStartTarget(owner));
    }
    fn retire_ordinary_start(&mut self, work: Box<super::ordinary_start::OrdinaryStartWork>) {
        self.retire_buffer(RetiredAudioBuffer::OrdinaryStart(work));
    }
    fn retire_ordinary_start_group(&mut self, group: super::ordinary_start::OrdinaryStartGroup) {
        self.retire_buffer(RetiredAudioBuffer::OrdinaryStartGroup(group));
    }
    fn retire_ordinary_start_publication(
        &mut self,
        work: Arc<super::ordinary_start::OrdinaryStartPublication>,
    ) {
        self.retire_buffer(RetiredAudioBuffer::OrdinaryStartPublication(work));
    }
    fn retire_pad_start_target(&mut self, owner: super::pad_start::PadStartCustody) {
        self.retire_buffer(RetiredAudioBuffer::PadStartTarget(owner));
    }
    fn retire_pitch_action(&mut self, work: Box<super::pitch_actions::PitchActionWork>) {
        self.retire_buffer(RetiredAudioBuffer::PitchAction(work));
    }
    fn retire_pitch_publication(
        &mut self,
        work: Arc<super::pitch_publication::PitchActionPublication>,
    ) {
        self.retire_buffer(RetiredAudioBuffer::PitchPublication(work));
    }
    fn retire_pad_pair(&mut self, work: Box<super::pad_pair::PadPairRtWork>) {
        self.retire_buffer(RetiredAudioBuffer::PadPair(work));
    }
    fn retire_hold_action(&mut self, work: Box<super::hold_action::HoldActionRtWork>) {
        self.retire_buffer(RetiredAudioBuffer::HoldAction(work));
    }
    fn retire_source_publication(
        &mut self,
        publication: super::input_runtime_binding::SourcePublication,
    ) {
        self.retire_buffer(RetiredAudioBuffer::SourcePublication(publication));
    }
    fn retire_bank_mutation(&mut self, work: Box<super::pad_copy::bank::BankMutationRtWork>) {
        self.retire_buffer(RetiredAudioBuffer::BankMutation(work));
    }
    fn retire_bank_publication(
        &mut self,
        work: Arc<super::pad_copy::bank::BankMutationPublication>,
    ) {
        self.retire_buffer(RetiredAudioBuffer::BankMutationPublication(work));
    }
    fn retire_pad_copy(&mut self, work: Box<super::pad_copy::PadCopyRtWork>) {
        self.retire_buffer(RetiredAudioBuffer::PadCopy(work));
    }
    fn retire_committed_stem_work(
        &mut self,
        work: Box<super::committed_stem_residency::CommittedStemRtWork>,
    ) {
        self.retire_buffer(RetiredAudioBuffer::CommittedStemWork(work));
    }
    fn retire_resident_capture(&mut self, capture: Arc<super::resident_seek::ResidentSeekCapture>) {
        self.retire_buffer(RetiredAudioBuffer::ResidentSeekCapture(capture));
    }
    fn retire_resident_cancellation(&mut self, cancelled: Arc<AtomicBool>) {
        self.retire_buffer(RetiredAudioBuffer::ResidentCancellation(cancelled));
    }
    fn retire_resident_transaction(
        &mut self,
        publication: Box<crate::messages::ResidentTransaction>,
    ) {
        self.retire_buffer(RetiredAudioBuffer::ResidentTransaction(publication));
    }
    fn retire_cold_adoption(&mut self, adoption: Arc<std::sync::atomic::AtomicU8>) {
        self.retire_buffer(RetiredAudioBuffer::ColdAdoption(adoption));
    }
    fn retire_accepted_timing_refresh(
        &mut self,
        refresh: Arc<super::accepted_timing_refresh::AcceptedTimingRefresh>,
    ) {
        self.retire_buffer(RetiredAudioBuffer::AcceptedTimingRefresh(refresh));
    }
    fn retire_global_playback_batch(
        &mut self,
        batch: Arc<super::global_playback_batch::GlobalPlaybackBatch>,
    ) {
        self.retire_buffer(RetiredAudioBuffer::GlobalPlaybackBatch(batch));
    }
    fn retire_sample(&mut self, sample: SampleBuffer) {
        self.retire_buffer(RetiredAudioBuffer::Sample(sample));
    }

    fn retire_prepared_stems(&mut self, stems: PreparedStemSet) {
        self.retire_buffer(RetiredAudioBuffer::PreparedStems(stems));
    }

    fn retire_constant_timing(&mut self, timing: PreparedConstantTiming) {
        self.retire_buffer(RetiredAudioBuffer::ConstantTiming(timing));
    }

    fn available_retirement_slots(&mut self) -> usize {
        self.flush_backlog();
        self.producer
            .slots()
            .saturating_add(RETIRED_AUDIO_BUFFER_BACKLOG_CAPACITY - self.backlog_len)
    }
}

pub(crate) struct AudioBufferRetirementWorker {
    running: Arc<AtomicBool>,
    join_handle: Option<JoinHandle<()>>,
}

impl AudioBufferRetirementWorker {
    fn join(&mut self) -> std::io::Result<()> {
        self.running.store(false, Ordering::Release);
        let Some(join_handle) = self.join_handle.take() else {
            return Ok(());
        };
        join_handle.join().map_err(|_| {
            std::io::Error::other(
                "audio retirement worker panicked; callback quiescence is unverified",
            )
        })
    }

    /// Only a successful join verifies the close queue and complete callback image.
    /// The caller has already closed its actual stream/callback owner off realtime.
    pub(crate) fn join_checked(mut self) -> std::io::Result<()> {
        self.join()
    }

    fn spawn<T: Send + 'static>(
        mut consumer: Consumer<RetiredAudioBuffer>,
        mut callback_close: Option<Consumer<Box<T>>>,
        callback_closed: Option<Arc<AtomicBool>>,
        worker_start: impl FnOnce() + Send + 'static,
    ) -> std::io::Result<Self> {
        let running = Arc::new(AtomicBool::new(true));
        let thread_running = running.clone();
        let join_handle = thread::Builder::new()
            .name("flitzis-audio-buffer-retirement".to_string())
            .spawn(move || {
                worker_start();
                // Stream drop may return before another thread destroys CPAL's closure. Keep
                // both consumers alive until its unique close producer has been dropped.
                while thread_running.load(Ordering::Acquire)
                    || !consumer.is_empty()
                    || callback_closed
                        .as_ref()
                        .is_some_and(|closed| !closed.load(Ordering::Acquire))
                    || callback_close
                        .as_ref()
                        .is_some_and(|close| !close.is_empty())
                {
                    let mut drained = false;
                    while let Ok(buffer) = consumer.pop() {
                        drop(buffer);
                        drained = true;
                    }
                    if let Some(close) = &mut callback_close {
                        if let Ok(state) = close.pop() {
                            // This also drops a full retirement backlog, queued commands,
                            // scheduled commands, Native voices and the one deferred payload.
                            drop(state);
                            drained = true;
                        }
                    }
                    if !drained {
                        thread::sleep(Duration::from_millis(5));
                    }
                }
            })?;
        Ok(Self {
            running,
            join_handle: Some(join_handle),
        })
    }
}

/// Unique callback owner. Its preallocated box leaves CPAL's destructor thread intact;
/// only the existing retirement worker destroys its state and any nested Native pins.
/// This producer has exactly one write for one initially empty slot, independent of
/// normal retirement queue/backlog capacity.
pub(crate) struct CallbackCloseOwner<T: Send + 'static> {
    state: Option<Box<T>>,
    close: Option<Producer<Box<T>>>,
    closed: Arc<AtomicBool>,
}

impl<T: Send + 'static> CallbackCloseOwner<T> {
    pub(crate) fn state_mut(&mut self) -> &mut T {
        self.state
            .as_deref_mut()
            .expect("live callback owns its state")
    }
}

impl<T: Send + 'static> Drop for CallbackCloseOwner<T> {
    fn drop(&mut self) {
        if let Some(state) = self.state.take() {
            if let Some(mut close) = self.close.take() {
                if let Err(PushError::Full(state)) = close.push(state) {
                    // A second write is structurally impossible. Even a broken internal
                    // invariant must not destroy the complete callback image on realtime.
                    std::mem::forget(state);
                }
                // The worker retains its consumer until after this producer is gone, so
                // neither the close ring nor a state payload can die on this thread.
                drop(close);
            } else {
                std::mem::forget(state);
            }
        }
        self.closed.store(true, Ordering::Release);
    }
}

fn create_audio_buffer_retirement_with_callback_start<T: Send + 'static>(
    build: impl FnOnce(RtAudioBufferRetirement) -> Box<T>,
    worker_start: impl FnOnce() + Send + 'static,
) -> std::io::Result<(CallbackCloseOwner<T>, AudioBufferRetirementWorker)> {
    let (producer, consumer) = RingBuffer::new(RETIRED_AUDIO_BUFFER_QUEUE_CAPACITY);
    let state = build(RtAudioBufferRetirement::new(producer));
    let (close, close_consumer) = RingBuffer::new(1);
    let closed = Arc::new(AtomicBool::new(false));
    // Failure drops the still-local state on the caller's control thread. No device
    // callback has been registered and no producer can outlive a missing worker.
    let worker = AudioBufferRetirementWorker::spawn(
        consumer,
        Some(close_consumer),
        Some(closed.clone()),
        worker_start,
    )?;
    Ok((
        CallbackCloseOwner {
            state: Some(state),
            close: Some(close),
            closed,
        },
        worker,
    ))
}

pub(crate) fn create_audio_buffer_retirement_with_callback<T: Send + 'static>(
    build: impl FnOnce(RtAudioBufferRetirement) -> Box<T>,
) -> std::io::Result<(CallbackCloseOwner<T>, AudioBufferRetirementWorker)> {
    create_audio_buffer_retirement_with_callback_start(build, || {})
}

#[cfg(test)]
pub(super) fn create_audio_buffer_retirement_with_callback_paused<T: Send + 'static>(
    build: impl FnOnce(RtAudioBufferRetirement) -> Box<T>,
    worker_start: impl FnOnce() + Send + 'static,
) -> std::io::Result<(CallbackCloseOwner<T>, AudioBufferRetirementWorker)> {
    create_audio_buffer_retirement_with_callback_start(build, worker_start)
}

impl Drop for AudioBufferRetirementWorker {
    fn drop(&mut self) {
        let _ = self.join();
    }
}

/// The productive stream and hardware-free callback owner use the same close barrier.
/// Returning successfully verifies actual close handoff and off-RT image destruction;
/// it does not join the independent capture file writer.
pub(crate) fn close_callback_and_join<T>(
    callback_owner: T,
    worker: AudioBufferRetirementWorker,
) -> std::io::Result<()> {
    drop(callback_owner);
    worker.join_checked()
}

#[cfg(test)]
pub(crate) fn create_audio_buffer_retirement()
-> (RtAudioBufferRetirement, AudioBufferRetirementWorker) {
    let (producer, consumer) = RingBuffer::new(RETIRED_AUDIO_BUFFER_QUEUE_CAPACITY);
    (
        RtAudioBufferRetirement::new(producer),
        AudioBufferRetirementWorker::spawn(consumer, None::<Consumer<Box<()>>>, None, || {})
            .expect("audio retirement worker unavailable"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn sample_with_weak() -> (SampleBuffer, std::sync::Weak<[f32]>) {
        let samples: Arc<[f32]> = Arc::from(vec![0.0_f32; 16].into_boxed_slice());
        let weak = Arc::downgrade(&samples);
        (
            SampleBuffer {
                residency: None,
                channels: 1,
                samples,
            },
            weak,
        )
    }

    #[test]
    fn retirement_queue_holds_sample_until_consumer_drops_it() {
        let (producer, mut consumer) = RingBuffer::new(1);
        let mut retirement = RtAudioBufferRetirement::new(producer);
        let (sample, weak) = sample_with_weak();

        retirement.retire_sample(sample);

        assert!(weak.upgrade().is_some());

        let retired = consumer.pop().unwrap();
        drop(retired);

        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn full_retirement_queue_uses_preallocated_backlog() {
        let (producer, _consumer) = RingBuffer::new(0);
        let mut retirement = RtAudioBufferRetirement::new(producer);
        let (sample, weak) = sample_with_weak();

        retirement.retire_sample(sample);

        assert!(weak.upgrade().is_some());
        assert_eq!(retirement.backlog_len, 1);
    }
}
