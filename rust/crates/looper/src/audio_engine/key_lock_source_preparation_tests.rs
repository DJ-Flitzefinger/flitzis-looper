//! Independent native/source reference and optional offline CSV measurements.
//!
//! The reference deliberately does not use SourcePlayback, SourceReadPlan, source-reader helpers,
//! or PreparedSourceStream to generate its input. Exact PCM equality establishes continuation at
//! an explicit output discard, not audible alignment or a production compensation rule.

use super::*;
use crate::audio_engine::source_reader::{
    ExplicitSeekMode, FrameRange, SourceReadPlan, StemRenderSelection, StemTransition,
};
use crate::messages::{PreparedStemSet, SampleBuffer, StemMixMode};
use std::fmt::Write as _;
use std::sync::Arc;

const RATES: [u32; 3] = [44_100, 48_000, 96_000];
const RATIOS: [f32; 5] = [0.5, 0.73, 1.0, 1.37, 2.0];
const PATTERNS: [(&str, &[usize]); 4] = [
    ("1", &[1]),
    ("64", &[64]),
    ("512", &[512]),
    ("irregular", &[64, 96, 257, 512, 31, 1]),
];

struct Fixture {
    sample: SampleBuffer,
    stems: Option<PreparedStemSet>,
    rate: u32,
    ratio: f32,
    region: FrameRange,
    start: usize,
    mode: ExplicitSeekMode,
    stem_mask: Option<u8>,
}

impl Fixture {
    fn request(&self, initial_frames: usize, discard: usize) -> SourcePreparation {
        let mut logical = SourcePlayback::new(self.start, self.mode, f64::from(self.ratio));
        logical.configure(self.sample.samples.len() / 2, self.region);
        logical.advance(initial_frames);
        let position = logical.position();
        let selection = self
            .stem_mask
            .map_or_else(StemRenderSelection::full_mix, |mask| {
                StemRenderSelection::from_state(StemMixMode::AllStems, 17, mask)
            });
        SourcePreparation {
            plan: SourceReadPlan {
                channels: 2,
                sample_frames: self.sample.samples.len() / 2,
                frame_pos: position.frame,
                loop_region: self.region,
                loop_period: None,
                seek_mode: position.seek_mode,
                selection,
                transition: StemTransition::default(),
            },
            logical,
            tempo_ratio: f64::from(self.ratio),
            discard_output_frames: discard,
            source_history: None,
        }
    }

    /// Closed-form absolute source coordinates, independent of the production source clock.
    fn reference_position(&self, output_frame: usize) -> (usize, f64, ExplicitSeekMode) {
        let distance = output_frame as f64 * f64::from(self.ratio);
        let whole = distance.floor() as usize;
        let (frame, mode) = self.reference_integer_position(whole);
        (frame, distance - whole as f64, mode)
    }

    fn reference_integer_position(&self, offset: usize) -> (usize, ExplicitSeekMode) {
        let source_frame = self.start + offset;
        let loop_length = self.region.end - self.region.start;
        let source_length = self.sample.samples.len() / 2;
        match self.mode {
            ExplicitSeekMode::Normal => (
                self.region.start + (source_frame - self.region.start) % loop_length,
                ExplicitSeekMode::Normal,
            ),
            ExplicitSeekMode::BeforeLoop if source_frame < self.region.start => {
                (source_frame, ExplicitSeekMode::BeforeLoop)
            }
            ExplicitSeekMode::BeforeLoop => (
                self.region.start + (source_frame - self.region.start) % loop_length,
                ExplicitSeekMode::Normal,
            ),
            ExplicitSeekMode::AfterLoop if source_frame < source_length => {
                (source_frame, ExplicitSeekMode::AfterLoop)
            }
            ExplicitSeekMode::AfterLoop => (
                self.region.start + (source_frame - source_length) % loop_length,
                ExplicitSeekMode::Normal,
            ),
        }
    }

    fn reference_sample(&self, output_frame: usize, channel: usize) -> f32 {
        let distance = output_frame as f64 * f64::from(self.ratio);
        let whole = distance.floor() as usize;
        let fraction = (distance - whole as f64) as f32;
        let left = self.reference_tap(whole, channel);
        if fraction == 0.0 {
            return left;
        }
        let right = self.reference_tap(whole + 1, channel);
        left + (right - left) * fraction
    }

    fn reference_tap(&self, offset: usize, channel: usize) -> f32 {
        let index = self.reference_integer_position(offset).0 * 2 + channel;
        if let Some(mask) = self.stem_mask {
            let stems = self.stems.as_ref().unwrap();
            let mut sample = 0.0;
            for component in 0..4 {
                if mask & (1 << component) != 0 {
                    sample += stems.stems[component].samples[index];
                }
            }
            sample
        } else {
            self.sample.samples[index]
        }
    }

    fn assert_position(&self, actual: FractionalSourcePosition, output_frame: usize) {
        let expected = self.reference_position(output_frame);
        assert_eq!(actual.frame, expected.0);
        assert_eq!(actual.fraction, expected.1);
        assert_eq!(actual.seek_mode, expected.2);
    }
}

fn component_sample(frame: usize, channel: usize, component: usize) -> f32 {
    let mut value = (frame as u32)
        .wrapping_mul(747_796_405)
        .wrapping_add((channel as u32 + 1) * 131 + component as u32 * 977);
    value = ((value >> ((value >> 28) + 4)) ^ value).wrapping_mul(277_803_737);
    value = (value >> 22) ^ value;
    (value as f64 / u32::MAX as f64 * 2.0 - 1.0) as f32 * 0.075
}

fn loop_fixture(rate: u32, ratio: f32, mode: ExplicitSeekMode, stem_mask: Option<u8>) -> Fixture {
    let stems: [SampleBuffer; 5] = std::array::from_fn(|component| {
        let samples = (0..1939)
            .flat_map(|frame| {
                (0..2).map(move |channel| {
                    if component == 4 {
                        (1..4)
                            .map(|part| component_sample(frame, channel, part))
                            .sum()
                    } else {
                        component_sample(frame, channel, component)
                    }
                })
            })
            .collect::<Vec<_>>();
        SampleBuffer {
            channels: 2,
            samples: Arc::from(samples),
        }
    });
    let full_mix = (0..1939 * 2)
        .map(|index| stems[..4].iter().map(|stem| stem.samples[index]).sum())
        .collect::<Vec<_>>();
    let sample = SampleBuffer {
        channels: 2,
        samples: Arc::from(full_mix),
    };
    let reference_samples = sample.samples.clone();
    Fixture {
        sample,
        stems: Some(PreparedStemSet {
            accepted_timing: None,
            reference_samples,
            publication: crate::audio_engine::prepared_source::PreparedSourcePermit::unrestricted(),
            source_version_hash: 17,
            sample_rate_hz: rate,
            channels: 2,
            frame_count: 1939,
            available_mask: 0x1f,
            stems,
        }),
        rate,
        ratio,
        region: FrameRange {
            start: 113,
            end: 1278,
        },
        start: match mode {
            ExplicitSeekMode::Normal => 1275,
            ExplicitSeekMode::BeforeLoop => 7,
            ExplicitSeekMode::AfterLoop => 1453,
        },
        mode,
        stem_mask,
    }
}

struct NativeReference {
    delay: usize,
    block_size: usize,
    output: Vec<Vec<f32>>,
}

fn native_reference(
    fixture: &Fixture,
    initial_frames: usize,
    retained_output: usize,
) -> NativeReference {
    let mut native = RubberBandLiveShifter::new(fixture.rate, 2).unwrap();
    // Independent initialization: reset sets the previous hop using the exact starting pitch.
    native
        .set_pitch_scale(1.0 / f64::from(fixture.ratio))
        .unwrap();
    native.reset_for_preparation();
    let delay = native.start_delay();
    let block_size = native.block_size();
    let total = (delay + retained_output).div_ceil(block_size) * block_size;
    let mut input = vec![vec![0.0; block_size]; 2];
    let mut shifted = vec![vec![0.0; block_size]; 2];
    let mut output = (0..2)
        .map(|_| Vec::with_capacity(total))
        .collect::<Vec<_>>();
    for offset in (0..total).step_by(block_size) {
        for (channel, input_channel) in input.iter_mut().enumerate() {
            for (frame, sample) in input_channel.iter_mut().enumerate() {
                *sample = fixture.reference_sample(initial_frames + offset + frame, channel);
            }
        }
        native.shift(&input, &mut shifted).unwrap();
        for channel in 0..2 {
            assert!(shifted[channel].iter().all(|sample| sample.is_finite()));
            output[channel].extend_from_slice(&shifted[channel]);
        }
    }
    NativeReference {
        delay,
        block_size,
        output,
    }
}

fn prove_prepared_continuation(
    fixture: &Fixture,
    initial_frames: usize,
    reference: &NativeReference,
    discard: usize,
    pattern: &[usize],
    output_frames: usize,
) -> Vec<Vec<f32>> {
    let request = fixture.request(initial_frames, discard);
    let unchanged_position = request.logical.position();
    let mut prepared = PreparedSourceStream::prepare(
        &fixture.sample,
        fixture.stems.as_ref(),
        request,
        RubberBandLiveShifter::new(fixture.rate, 2).unwrap(),
    )
    .unwrap();
    assert_eq!(request.logical.position(), unchanged_position);
    assert_eq!(prepared.native_delay(), reference.delay);
    assert_eq!(prepared.discard_frames(), discard);
    let block = reference.block_size;
    let feed_frames = (discard + block - 1).div_ceil(block) * block;
    let retained = feed_frames - discard;
    assert_eq!(prepared.prepared_feed_frames(), feed_frames);
    assert_eq!(prepared.retained_frames(), retained);
    assert!((block - 1..=2 * block - 2).contains(&retained));
    fixture.assert_position(prepared.logical_position(), initial_frames);
    fixture.assert_position(prepared.feed_position(), initial_frames + feed_frames);
    let mut collected = (0..2)
        .map(|_| Vec::with_capacity(output_frames))
        .collect::<Vec<_>>();
    let mut elapsed = 0;
    let mut partition = 0;
    while elapsed < output_frames {
        let frames = pattern[partition % pattern.len()].min(output_frames - elapsed);
        let output = prepared.render(frames).unwrap();
        for channel in 0..2 {
            assert_eq!(
                &output[channel][..frames],
                &reference.output[channel][discard + elapsed..discard + elapsed + frames],
                "prepared continuation diverged: rate={}, ratio={}, mode={:?}, mask={:?}, initial={}, elapsed={}, pattern={:?}",
                fixture.rate,
                fixture.ratio,
                fixture.mode,
                fixture.stem_mask,
                initial_frames,
                elapsed,
                pattern,
            );
            collected[channel].extend_from_slice(&output[channel][..frames]);
        }
        elapsed += frames;
        partition += 1;
        fixture.assert_position(prepared.logical_position(), initial_frames + elapsed);
        fixture.assert_position(
            prepared.feed_position(),
            initial_frames + feed_frames + elapsed,
        );
        assert_eq!(prepared.pending_input_frames(), elapsed % block);
        assert_eq!(prepared.fifo_occupancy(), retained - elapsed % block);
    }
    collected
}

#[test]
fn prepared_fifo_and_continuation_match_independent_native_source_reference() {
    let mut cases = 0;
    for rate in RATES {
        for ratio in RATIOS {
            for mode in [
                ExplicitSeekMode::Normal,
                ExplicitSeekMode::BeforeLoop,
                ExplicitSeekMode::AfterLoop,
            ] {
                for mask in [None, Some(0b1010)] {
                    let fixture = loop_fixture(rate, ratio, mode, mask);
                    for initial_frames in [0, 17] {
                        let reference = native_reference(&fixture, initial_frames, 6147);
                        for (_, pattern) in PATTERNS {
                            prove_prepared_continuation(
                                &fixture,
                                initial_frames,
                                &reference,
                                reference.delay,
                                pattern,
                                6147,
                            );
                            cases += 1;
                        }
                    }
                }
            }
        }
    }
    assert_eq!(cases, 720);
}

#[test]
fn explicit_discard_endpoints_preserve_exact_native_output_index() {
    let fixture = loop_fixture(48_000, 1.37, ExplicitSeekMode::Normal, Some(0b1010));
    let reference = native_reference(&fixture, 17, 3073);
    let block = reference.block_size;
    for discard in [0, 1, block - 1, block, block + 1, reference.delay + 17] {
        for (_, pattern) in PATTERNS {
            prove_prepared_continuation(&fixture, 17, &reference, discard, pattern, 2049);
        }
    }
}

#[test]
fn prepared_all_component_stems_match_the_same_full_mix_reference() {
    for rate in RATES {
        for ratio in RATIOS {
            let full_mix = loop_fixture(rate, ratio, ExplicitSeekMode::Normal, None);
            let stems = loop_fixture(rate, ratio, ExplicitSeekMode::Normal, Some(0b1111));
            let reference = native_reference(&full_mix, 17, 3077);
            for (_, pattern) in PATTERNS {
                prove_prepared_continuation(&stems, 17, &reference, reference.delay, pattern, 3077);
            }
        }
    }
}

struct Response {
    peak_frame: usize,
    peak: f32,
    onset: Option<usize>,
    significant_onset: Option<usize>,
}

fn response(samples: &[f32]) -> Response {
    assert!(!samples.is_empty());
    assert!(samples.iter().all(|sample| sample.is_finite()));
    // Select the first maximum, including a flat retained/clipped response.
    let mut peak_frame = 0;
    let mut peak = 0.0_f32;
    for (frame, sample) in samples.iter().enumerate() {
        if sample.abs() > peak {
            peak_frame = frame;
            peak = sample.abs();
        }
    }
    let detected = peak > 1.0e-7;
    Response {
        peak_frame,
        peak,
        onset: detected.then(|| {
            samples
                .iter()
                .position(|sample| sample.abs() > 1.0e-7)
                .unwrap()
        }),
        significant_onset: detected.then(|| {
            samples
                .iter()
                .position(|sample| sample.abs() >= peak * 0.01)
                .unwrap()
        }),
    }
}

struct ProbeCase<'a> {
    rate: u32,
    ratio: f32,
    pattern: &'a str,
    marker: usize,
}

#[derive(Default)]
struct CsvReport {
    rows: String,
}

impl CsvReport {
    fn emit(
        &mut self,
        case: &ProbeCase<'_>,
        kind: &str,
        metric: &str,
        value: impl std::fmt::Display,
        unit: &str,
    ) {
        writeln!(
            self.rows,
            "{kind},{},{},{},{},{metric},{value},{unit}",
            case.rate, case.ratio, case.pattern, case.marker
        )
        .unwrap();
    }

    fn emit_response(
        &mut self,
        case: &ProbeCase<'_>,
        kind: &str,
        measured: &Response,
        offset: i64,
    ) {
        self.emit(
            case,
            kind,
            "detected",
            usize::from(measured.onset.is_some()),
            "bool",
        );
        self.emit(case, kind, "peak_amplitude", measured.peak, "linear");
        if let Some(onset) = measured.onset {
            self.emit(
                case,
                kind,
                "onset_residual",
                onset as i64 - offset,
                "frames",
            );
            self.emit(
                case,
                kind,
                "one_percent_onset_residual",
                measured.significant_onset.unwrap() as i64 - offset,
                "frames",
            );
            self.emit(
                case,
                kind,
                "peak_residual",
                measured.peak_frame as i64 - offset,
                "frames",
            );
        }
    }

    fn save_if_requested(self) {
        if let Some(path) = std::env::var_os("FLITZIS_KEY_LOCK_SOURCE_PROBE_CSV") {
            let path = std::path::PathBuf::from(path);
            assert!(path.is_absolute(), "probe CSV path must be absolute");
            let header =
                "kind,sample_rate_hz,tempo_ratio,callback_pattern,marker,metric,value,unit\n";
            std::fs::write(path, format!("{header}{}", self.rows)).unwrap();
        }
    }
}

fn impulse_fixture(rate: u32, ratio: f32, marker: usize, output_frames: usize) -> Fixture {
    // A single immutable source impulse: interpolation cannot relocate it per callback.
    let source_marker = (marker as f64 * f64::from(ratio)).round() as usize;
    let source_frames = ((output_frames + 16_384) as f64 * f64::from(ratio)).ceil() as usize + 2;
    let mut samples = vec![0.0; source_frames * 2];
    samples[source_marker * 2] = 1.0;
    samples[source_marker * 2 + 1] = 0.5;
    Fixture {
        sample: SampleBuffer {
            channels: 2,
            samples: Arc::from(samples),
        },
        stems: None,
        rate,
        ratio,
        region: FrameRange {
            start: 0,
            end: source_frames,
        },
        start: 0,
        mode: ExplicitSeekMode::Normal,
        stem_mask: None,
    }
}

#[test]
fn exact_ratio_discard_reports_startup_settled_and_off_block_transients() {
    let mut csv = CsvReport::default();
    let mut cases = 0;
    let mut clipped_peaks = 0;
    for rate in RATES {
        for ratio in RATIOS {
            for marker in [0, 8192, 8209] {
                let output_frames = marker + rate as usize / 3;
                let fixture = impulse_fixture(rate, ratio, marker, output_frames);
                let reference = native_reference(&fixture, 0, output_frames);
                let dry = (0..output_frames)
                    .map(|frame| fixture.reference_sample(frame, 0))
                    .collect::<Vec<_>>();
                let dry_response = response(&dry);
                assert!(dry_response.onset.is_some());
                let raw = &reference.output[0][..reference.delay + output_frames];
                let raw_response = response(raw);
                assert!(raw_response.onset.is_some());
                let total_energy = raw
                    .iter()
                    .map(|value| f64::from(*value).powi(2))
                    .sum::<f64>();
                let discarded_energy = raw[..reference.delay]
                    .iter()
                    .map(|value| f64::from(*value).powi(2))
                    .sum::<f64>();
                let peak_retained = raw_response.peak_frame >= reference.delay;
                clipped_peaks += usize::from(!peak_retained);
                for (pattern_name, pattern) in PATTERNS {
                    let retained = prove_prepared_continuation(
                        &fixture,
                        0,
                        &reference,
                        reference.delay,
                        pattern,
                        output_frames,
                    );
                    let case = ProbeCase {
                        rate,
                        ratio,
                        pattern: pattern_name,
                        marker,
                    };
                    csv.emit(
                        &case,
                        "accounting",
                        "explicit_discard",
                        reference.delay,
                        "frames",
                    );
                    csv.emit(
                        &case,
                        "accounting",
                        "native_nominal_delay",
                        reference.delay,
                        "frames",
                    );
                    csv.emit(
                        &case,
                        "accounting",
                        "dry_reference_frame",
                        dry_response.peak_frame,
                        "frames",
                    );
                    csv.emit(
                        &case,
                        "accounting",
                        "native_block_size",
                        reference.block_size,
                        "frames",
                    );
                    csv.emit(&case, "accounting", "exact_reference_equal", 1, "bool");
                    csv.emit_response(&case, "dry", &dry_response, dry_response.peak_frame as i64);
                    csv.emit_response(
                        &case,
                        "raw_translated",
                        &raw_response,
                        (reference.delay + dry_response.peak_frame) as i64,
                    );
                    csv.emit(
                        &case,
                        "clipping",
                        "original_peak_retained",
                        usize::from(peak_retained),
                        "bool",
                    );
                    csv.emit(
                        &case,
                        "clipping",
                        "discarded_energy_fraction",
                        discarded_energy / total_energy,
                        "fraction",
                    );
                    csv.emit_response(
                        &case,
                        "retained",
                        &response(&retained[0]),
                        dry_response.peak_frame as i64,
                    );
                    cases += 1;
                }
            }
        }
    }
    assert_eq!(cases, 180);
    // Demonstrates why exact-reference equality cannot be accepted as onset compensation.
    assert!(
        clipped_peaks > 0,
        "fixture must expose loss of an original startup peak"
    );
    csv.save_if_requested();
}

#[path = "key_lock_source_history_probe.rs"]
mod history_probe;
