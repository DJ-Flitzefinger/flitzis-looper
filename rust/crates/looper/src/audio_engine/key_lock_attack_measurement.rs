//! Worker-only observation of the actual source/native contract for one captured endpoint.
//!
//! The reference at H is a captured endpoint SOURCE-PATTERN CANDIDATE, declared before any
//! wet content is inspected. It is not an independently labelled principal attack. Neither
//! a spectral correspondence nor its lag accepts musical C, emission, perceptual precision,
//! isolated-response retention, click safety or device/listening acceptance.
//!
//! The temporary Native and all PCM below are worker-local. The receipt contains only fixed
//! scalars: it cannot retain a source, grant admission or mutate the eventual Native/FIFOs.

use super::key_lock_attack_reference::{
    AttackCorrespondence, AttackCorrespondenceCandidate, AttackLagRange, REVISION,
    SourceAttackReference, compare_attack_reference,
};
use super::native_history_permit::NativeHistoryPermitIdentity;
use super::native_source_coverage::{
    complete_view_available, normal_loop_feed_available_with_readers,
};
use super::pitch_intent::PitchIntent;
use super::prepared_native_history::{
    NativeHistoryRequest, SourceReadCheckpoint, launch_history_output_frames, launch_source_rebase,
};
use super::productive_source_history::ProductiveSourceBinding;
use super::rubberband_backend::RubberBandLiveShifter;
use super::source_feed_history::SourceFeedContext;
use super::source_reader::{FrameRange, StemReadSets};

const MAX_CHANNELS: usize = 8;
const MAX_FRAMES: usize = 262_144;
const FEATURE_BIN_HZ: f64 = 50.0;
const FEATURE_STEP_SECONDS: f64 = 0.001;
const CAPTURED_ENDPOINT_REFERENCE_KIND: &str = "captured-endpoint-source-pattern-candidate-v1";
const PHYSICAL_BIRTH_REFERENCE_KIND: &str = "physical-birth-source-pattern-candidate-v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MeasurementContext {
    SourceHistory,
    PhysicalBirth,
}

impl MeasurementContext {
    fn reference_kind(self) -> &'static str {
        match self {
            Self::SourceHistory => CAPTURED_ENDPOINT_REFERENCE_KIND,
            Self::PhysicalBirth => PHYSICAL_BIRTH_REFERENCE_KIND,
        }
    }
}

/// An observation of one original endpoint request, never accepted attack compensation.
///
/// An exact Unavailable receipt is valid metadata and supplies no guessed zero-lag fallback.
/// The caller must independently keep all ordinary source/action/adoption guards.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PreparedAttackMeasurement {
    context: MeasurementContext,
    contract: Option<RequestContract>,
    geometry: Option<MeasurementGeometry>,
    pub(crate) reference_kind: &'static str,
    pub(crate) result: AttackCorrespondence,
}

#[derive(Clone, Copy, Debug)]
struct RequestContract {
    permit: NativeHistoryPermitIdentity,
    binding: ProductiveSourceBinding,
    readers: SourceFeedContext,
    logical: SourceReadCheckpoint,
    actual: Option<SourceReadCheckpoint>,
    pitch: PitchIntent,
    key_lock: bool,
    target_output_frame: u64,
    request_id: u64,
    epoch: u64,
    finite_coverage: bool,
}

impl RequestContract {
    fn capture(request: &NativeHistoryRequest) -> Option<Self> {
        if !scalar_layout_available(request) {
            return None;
        }
        Some(Self {
            permit: request.permit.scalar_identity(),
            binding: request.binding,
            readers: reader_context(request),
            logical: SourceReadCheckpoint {
                playback: request.playback,
                plan: request.plan,
            },
            actual: request.actual_feed,
            pitch: request.pitch_intent,
            key_lock: request.key_lock,
            target_output_frame: request.target_output_frame,
            request_id: request.request_id,
            epoch: request.epoch,
            finite_coverage: request.coverage.is_some(),
        })
    }

    fn matches_request(&self, request: &NativeHistoryRequest) -> bool {
        scalar_layout_available(request)
            && self.permit == request.permit.scalar_identity()
            && self.binding == request.binding
            && self.binding
                == ProductiveSourceBinding::new(
                    &request.sample,
                    request.binding.sample_rate_hz,
                    request.binding.accepted,
                )
            && self.readers == reader_context(request)
            && self.logical.matches_exact(SourceReadCheckpoint {
                playback: request.playback,
                plan: request.plan,
            })
            && (match (self.actual, request.actual_feed) {
                (None, None) => true,
                (Some(original), Some(current)) => original.matches_exact(current),
                _ => false,
            })
            && self.pitch == request.pitch_intent
            && self.key_lock == request.key_lock
            && self.target_output_frame == request.target_output_frame
            && self.request_id == request.request_id
            && self.epoch == request.epoch
            && self.finite_coverage == request.coverage.is_some()
            && request
                .permit
                .current(&request.sample, request.binding.sample_rate_hz)
            && request.permit.matches_projection(request.binding.accepted)
    }
}

fn reader_context(request: &NativeHistoryRequest) -> SourceFeedContext {
    SourceFeedContext::new(
        request.binding,
        request
            .plan
            .required_read_sets(StemReadSets::new(request.stems.as_ref(), None)),
    )
}

fn scalar_layout_available(request: &NativeHistoryRequest) -> bool {
    let sample_layout = |sample: &crate::messages::SampleBuffer| {
        sample.channels != 0
            && !sample.samples.is_empty()
            && sample.samples.len().is_multiple_of(sample.channels)
            && sample.frame_count().checked_mul(sample.channels).is_some()
            && sample
                .resident_start()
                .checked_add(sample.samples.len() / sample.channels)
                .is_some()
    };
    sample_layout(&request.sample)
        && request.stems.as_ref().is_none_or(|stems| {
            stems.channels != 0
                && !stems.reference_samples.is_empty()
                && stems.reference_samples.len().is_multiple_of(stems.channels)
                && stems.frame_count.checked_mul(stems.channels).is_some()
                && stems.reference_residency.as_ref().is_none_or(|view| {
                    view.start_frame
                        .checked_add(stems.reference_samples.len() / stems.channels)
                        .is_some()
                })
                && stems.stems.iter().all(sample_layout)
        })
}

#[derive(Clone, Copy, Debug)]
struct MeasurementGeometry {
    context: MeasurementContext,
    launch_catchup_frames: u64,
    source_rebase: Option<(usize, SourceReadCheckpoint)>,
    history_output_frames: usize,
    native_block_frames: usize,
    capture_frames: usize,
    silent_prefix_frames: usize,
    source_origin: SourceReadCheckpoint,
    reference: SourceAttackReference,
    search: AttackLagRange,
    feature_hop_frames: usize,
    feature_window_frames: usize,
}

impl MeasurementGeometry {
    fn new(
        request: &NativeHistoryRequest,
        history: usize,
        block: usize,
        catchup_frames: u64,
    ) -> Option<Self> {
        Self::in_context(
            request,
            history,
            block,
            catchup_frames,
            MeasurementContext::SourceHistory,
        )
    }

    fn in_context(
        request: &NativeHistoryRequest,
        history: usize,
        block: usize,
        catchup_frames: u64,
        context: MeasurementContext,
    ) -> Option<Self> {
        let rate = request.binding.sample_rate_hz;
        if !(8_000..=768_000).contains(&rate)
            || history == 0
            || history > MAX_FRAMES
            || block == 0
            || block > MAX_FRAMES
            || (context == MeasurementContext::PhysicalBirth
                && (catchup_frames != 0 || !supported_request(request)))
        {
            return None;
        }
        // Exactly the producer's fixed DetectionFunction geometry, without constructing
        // an FFT on later callback-side receipt validation.
        let feature_hop_frames = (f64::from(rate) * FEATURE_STEP_SECONDS) as usize;
        let feature_window_frames =
            ((f64::from(rate) / FEATURE_BIN_HZ) as usize).checked_next_power_of_two()?;
        let support_window =
            feature_window_frames.checked_add(feature_hop_frames.checked_mul(2)?)?;
        if feature_hop_frames == 0 {
            return None;
        }
        // The reference and complete source neighborhood are frozen before wet inspection.
        // This identifies only the captured endpoint's local source pattern, not which
        // event within that pattern is the Human's principal/perceived recorded attack.
        let reference = SourceAttackReference {
            frame: history,
            support: FrameRange {
                start: history.checked_sub(support_window)?,
                end: history.checked_add(support_window.checked_mul(2)?)?,
            },
        };
        let search = AttackLagRange {
            min: 0,
            max: i64::try_from(history).ok()?,
        };
        let required = reference
            .support
            .end
            .checked_add(history)?
            .checked_add(support_window)?;
        let capture_frames = required.div_ceil(block).checked_mul(block)?;
        if capture_frames > MAX_FRAMES {
            return None;
        }
        let (playback, silent_prefix_frames) = match context {
            MeasurementContext::SourceHistory => {
                request.playback.history_before_constant(history)?
            }
            // Negative birth-relative input is silence. The exact frozen endpoint
            // supplies the first physical source sample after that prefix.
            MeasurementContext::PhysicalBirth => (request.playback, history),
        };
        if silent_prefix_frames > history {
            return None;
        }
        let position = playback.position();
        let source_origin = SourceReadCheckpoint {
            playback,
            plan: super::source_reader::SourceReadPlan {
                frame_pos: position.frame,
                seek_mode: position.seek_mode,
                ..request.plan
            },
        };
        let source_rebase = match context {
            MeasurementContext::SourceHistory => {
                launch_source_rebase(request, history, catchup_frames).ok()?
            }
            MeasurementContext::PhysicalBirth => None,
        };
        if source_rebase.is_some_and(|(boundary, _)| boundary % block != 0) {
            // A raw Native input block cannot silently cross an unsupported capture epoch.
            return None;
        }
        source_origin.is_consistent().then_some(Self {
            context,
            launch_catchup_frames: catchup_frames,
            source_rebase,
            history_output_frames: history,
            native_block_frames: block,
            capture_frames,
            silent_prefix_frames,
            source_origin,
            reference,
            search,
            feature_hop_frames,
            feature_window_frames,
        })
    }

    fn matches_request(self, request: &NativeHistoryRequest) -> bool {
        let Some(expected) = Self::in_context(
            request,
            self.history_output_frames,
            self.native_block_frames,
            self.launch_catchup_frames,
            self.context,
        ) else {
            return false;
        };
        self.capture_frames == expected.capture_frames
            && match (self.source_rebase, expected.source_rebase) {
                (None, None) => true,
                (Some((at, checkpoint)), Some((other_at, other))) => {
                    at == other_at && checkpoint.matches_exact(other)
                }
                _ => false,
            }
            && self.silent_prefix_frames == expected.silent_prefix_frames
            && self.source_origin.matches_exact(expected.source_origin)
            && self.reference.frame == expected.reference.frame
            && self.reference.support == expected.reference.support
            && self.search == expected.search
            && self.feature_hop_frames == expected.feature_hop_frames
            && self.feature_window_frames == expected.feature_window_frames
    }

    fn validates_candidate(self, candidate: AttackCorrespondenceCandidate) -> bool {
        let Ok(source_frame) = i64::try_from(self.reference.frame) else {
            return false;
        };
        let support_radius = self.feature_window_frames + 3 * self.feature_hop_frames;
        candidate.source_frame == self.reference.frame
            && source_frame.checked_add(candidate.lag_frames) == Some(candidate.wet_frame)
            && (self.search.min..=self.search.max).contains(&candidate.lag_frames)
            && candidate.lag_frames % self.feature_hop_frames as i64 == 0
            && candidate.lag_support.min >= self.search.min
            && candidate.lag_support.max <= self.search.max
            && candidate.lag_support.min <= candidate.lag_frames - support_radius as i64
            && candidate.lag_support.max >= candidate.lag_frames + support_radius as i64
            && candidate.score.is_finite()
            && (0.75..=1.0).contains(&candidate.score)
            && candidate.runner_up_score.is_finite()
            && (-1.0..=1.0).contains(&candidate.runner_up_score)
            && candidate.score - candidate.runner_up_score >= 0.10
            && candidate.feature_hop_frames == self.feature_hop_frames
            && candidate.feature_window_frames == self.feature_window_frames
            && candidate.revision == REVISION
    }
}

impl PreparedAttackMeasurement {
    /// Fixed scalar/current-authority validation; this neither reruns FFT/Native nor accepts C.
    pub(crate) fn matches_request(&self, request: &NativeHistoryRequest) -> bool {
        self.matches_context(request, MeasurementContext::SourceHistory)
    }

    /// A genuine physical-birth receipt cannot be replaced by wrapped source history.
    /// Unavailable birth metadata remains valid only for its exact original request.
    pub(crate) fn matches_birth_request(&self, request: &NativeHistoryRequest) -> bool {
        self.matches_context(request, MeasurementContext::PhysicalBirth)
    }

    fn matches_context(&self, request: &NativeHistoryRequest, context: MeasurementContext) -> bool {
        if self.context != context
            || self.reference_kind != context.reference_kind()
            || self
                .contract
                .as_ref()
                .is_none_or(|contract| !contract.matches_request(request))
        {
            return false;
        }
        let Some(geometry) = self.geometry else {
            return matches!(self.result, AttackCorrespondence::Unavailable);
        };
        if geometry.context != context || !geometry.matches_request(request) {
            return false;
        }
        match self.result {
            AttackCorrespondence::SupportedCandidate(candidate) => {
                geometry.validates_candidate(candidate)
            }
            AttackCorrespondence::Ambiguous
            | AttackCorrespondence::Absent
            | AttackCorrespondence::Unavailable => true,
        }
    }
}

/// Measure one real admitted request using a separate temporary Native, off realtime.
///
/// Unsupported history/rate/reader geometry remains Unavailable. In particular an actual
/// input offset or live source/rate ramp is not relabelled as an endpoint reference.
#[cfg(test)]
pub(crate) fn measure_attack_reference_for_request(
    request: &NativeHistoryRequest,
) -> PreparedAttackMeasurement {
    measure_attack_reference_for_launch(request, 0)
}

pub(crate) fn measure_attack_reference_for_launch(
    request: &NativeHistoryRequest,
    catchup_frames: u64,
) -> PreparedAttackMeasurement {
    measure_in_context(request, catchup_frames, MeasurementContext::SourceHistory)
}

/// Observe a fresh physical beginning with zero negative birth-relative input.
/// This worker-only candidate does not choose or accept a musical attack anchor.
pub(crate) fn measure_attack_reference_for_birth(
    request: &NativeHistoryRequest,
) -> PreparedAttackMeasurement {
    measure_in_context(request, 0, MeasurementContext::PhysicalBirth)
}

fn measure_in_context(
    request: &NativeHistoryRequest,
    catchup_frames: u64,
    context: MeasurementContext,
) -> PreparedAttackMeasurement {
    let contract = RequestContract::capture(request);
    let mut measurement = PreparedAttackMeasurement {
        context,
        contract,
        geometry: None,
        reference_kind: context.reference_kind(),
        result: AttackCorrespondence::Unavailable,
    };
    let Some(contract) = contract else {
        return measurement;
    };
    let captured = match context {
        MeasurementContext::SourceHistory => capture_request(request, &contract, catchup_frames),
        MeasurementContext::PhysicalBirth => {
            capture_in_context(request, &contract, 0, MeasurementContext::PhysicalBirth)
        }
    };
    let Some(captured) = captured else {
        return measurement;
    };
    measurement.geometry = Some(captured.geometry);
    if !contract.matches_request(request) {
        return measurement;
    }
    let result = compare_attack_reference(
        &captured.dry,
        &captured.wet,
        request.binding.sample_rate_hz,
        captured.geometry.reference,
        captured.geometry.search,
    );
    if contract.matches_request(request) {
        measurement.result = result;
        if !measurement.matches_context(request, context) {
            measurement.result = AttackCorrespondence::Unavailable;
        }
    }
    measurement
}

struct CapturedReference {
    geometry: MeasurementGeometry,
    dry: Vec<Vec<f32>>,
    wet: Vec<Vec<f32>>,
}

fn supported_request(request: &NativeHistoryRequest) -> bool {
    let checkpoint = SourceReadCheckpoint {
        playback: request.playback,
        plan: request.plan,
    };
    let ratio = request.playback.tempo_ratio();
    (1..=MAX_CHANNELS).contains(&request.plan.channels)
        && (8_000..=768_000).contains(&request.binding.sample_rate_hz)
        && !request.plan.transition.is_active()
        && checkpoint.is_consistent()
        && request.actual_feed_checkpoint().matches_exact(checkpoint)
        && ratio.is_finite()
        && ratio > 0.0
        && ratio.to_bits() == request.playback.rate_target().to_bits()
}

fn source_read_available(
    request: &NativeHistoryRequest,
    checkpoint: SourceReadCheckpoint,
    readers: StemReadSets<'_>,
) -> bool {
    let rate = request.binding.sample_rate_hz;
    normal_loop_feed_available_with_readers(
        &request.sample,
        readers,
        rate,
        request.binding.accepted,
        checkpoint.plan,
        &checkpoint.playback,
    ) && request.coverage.as_ref().map_or_else(
        || complete_view_available(&request.sample),
        |coverage| {
            coverage.matches_with_readers(
                &request.sample,
                readers,
                rate,
                request.binding.accepted,
                checkpoint.plan,
                &checkpoint.playback,
                true,
            )
        },
    )
}

fn capture_request(
    request: &NativeHistoryRequest,
    contract: &RequestContract,
    catchup_frames: u64,
) -> Option<CapturedReference> {
    capture_in_context(
        request,
        contract,
        catchup_frames,
        MeasurementContext::SourceHistory,
    )
}

fn capture_in_context(
    request: &NativeHistoryRequest,
    contract: &RequestContract,
    catchup_frames: u64,
    context: MeasurementContext,
) -> Option<CapturedReference> {
    if !contract.matches_request(request) || !supported_request(request) {
        return None;
    }
    let readers = request
        .plan
        .required_read_sets(StemReadSets::new(request.stems.as_ref(), None));
    if !source_read_available(request, contract.logical, readers) {
        return None;
    }
    let channels = request.plan.channels;
    let pitch = request
        .pitch_intent
        .native_pitch_scale(request.playback.tempo_ratio(), request.key_lock)
        .ok()?;
    let mut native = RubberBandLiveShifter::new(request.binding.sample_rate_hz, channels).ok()?;
    // Identical exact initial pitch/reset/order to the real preparation path. In particular
    // the expanded-pitch silent initialization remains part of prepare_exact_pitch itself.
    native.prepare_exact_pitch(pitch).ok()?;
    let history = launch_history_output_frames(&native, request, catchup_frames).ok()?;
    let block = native.block_size();
    let geometry = match context {
        MeasurementContext::SourceHistory => {
            MeasurementGeometry::new(request, history, block, catchup_frames)?
        }
        MeasurementContext::PhysicalBirth => {
            MeasurementGeometry::in_context(request, history, block, catchup_frames, context)?
        }
    };
    let mut checkpoint = geometry.source_origin;
    let mut silent = geometry.silent_prefix_frames;
    let mut input = vec![vec![0.0; block]; channels];
    let mut output = vec![vec![0.0; block]; channels];
    let mut dry = (0..channels)
        .map(|_| Vec::with_capacity(geometry.capture_frames))
        .collect::<Vec<_>>();
    let mut wet = (0..channels)
        .map(|_| Vec::with_capacity(geometry.capture_frames))
        .collect::<Vec<_>>();
    for block_index in 0..geometry.capture_frames / block {
        if !contract.matches_request(request) {
            return None;
        }
        if let Some((boundary, original)) = geometry.source_rebase
            && block_index * block == boundary
        {
            if silent != 0 {
                return None;
            }
            checkpoint = original;
        }
        let prefix = silent.min(block);
        let frames = block - prefix;
        if frames != 0 {
            if !source_read_available(request, checkpoint, readers) {
                return None;
            }
            checkpoint
                .plan
                .fill_fractional_buffers_checked_with_readers(
                    &request.sample,
                    readers,
                    &checkpoint.playback,
                    &mut input,
                    frames,
                )
                .ok()?;
            if !checkpoint.playback.advance(frames) {
                return None;
            }
            let position = checkpoint.playback.position();
            checkpoint.plan.frame_pos = position.frame;
            checkpoint.plan.seek_mode = position.seek_mode;
        }
        for channel in &mut input {
            channel.copy_within(0..frames, prefix);
            channel[..prefix].fill(0.0);
            if channel.iter().any(|sample| !sample.is_finite()) {
                return None;
            }
        }
        // Physical-prefix zeros consume raw frames but never advance the real source clock.
        silent -= prefix;
        native.shift(&input, &mut output).ok()?;
        for channel in 0..channels {
            if output[channel].iter().any(|sample| !sample.is_finite()) {
                return None;
            }
            dry[channel].extend_from_slice(&input[channel]);
            wet[channel].extend_from_slice(&output[channel]);
        }
    }
    contract
        .matches_request(request)
        .then_some(CapturedReference { geometry, dry, wet })
}

#[cfg(test)]
#[path = "key_lock_attack_measurement_tests.rs"]
mod tests;
