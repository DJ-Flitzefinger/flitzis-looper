// Original-qualified mapped Native output, without public admission or a fitted musical C.
// Technical assertions and unchanged acoustic measurements have different success criteria.

use super::*;
use crate::audio_engine::audio_quality_metrics::{
    MAX_CAPTURE_TAIL_ENERGY, MAX_DISCARDED_ENERGY, TIMING_BUDGET_MS, energy_times,
    maximum_energy_discard, stereo_energy, stereo_peak,
};
use crate::audio_engine::pitch_intent::PitchIntent;
use crate::audio_engine::prepared_native_history::{
    MappedNativeInput, NativeAdapterState, NativeHistoryRequest, RetainedNativeStartCut,
    SourceReadCheckpoint,
};
use crate::audio_engine::rubberband_backend::{
    RubberBandLiveOptions, RubberBandLiveShifter, RubberBandLiveStateOpaque,
};
use crate::audio_engine::source_beatmap::render_comparison_tests::{
    CanonicalFixture, OUTPUT, SOURCE, SPAN, Signal, pcm_hash,
};
use crate::audio_engine::source_feed_history::SourceReadDomain;
use crate::audio_engine::source_playback::SourcePlayback;
use crate::audio_engine::stretch_processor::{ProductiveSourceFeed, StretchProcessor};

const BLOCK: usize = 512;
const ANCHORS: [usize; 5] = [0, 16_384, 49_152, 114_688, SOURCE];
const RATES: [f64; 4] = [0.5, 1.0, 2.0, 1.5];
const CALLERS: [(&str, &[usize]); 2] = [("512", &[512]), ("irregular", &[73, 511, 7, 512, 127, 3])];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NativeOutputProfile {
    Short,
    Medium,
}

impl NativeOutputProfile {
    fn label(self) -> &'static str {
        match self {
            Self::Short => "short-together",
            Self::Medium => "medium-together-classification-readahead",
        }
    }

    fn options(self) -> RubberBandLiveOptions {
        match self {
            Self::Short => RubberBandLiveOptions::default(),
            Self::Medium => {
                RubberBandLiveOptions::WINDOW_MEDIUM.union(RubberBandLiveOptions::CHANNELS_TOGETHER)
            }
        }
    }

    fn native(self) -> RubberBandLiveShifter {
        match self {
            Self::Short => RubberBandLiveShifter::new(RATE, 2).unwrap(),
            Self::Medium => RubberBandLiveShifter::with_options(RATE, 2, self.options()).unwrap(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum GeometryProfile {
    OriginalB5NearestRounded,
    DeclaredAdjacentSupported,
}

impl GeometryProfile {
    fn label(self) -> &'static str {
        match self {
            Self::OriginalB5NearestRounded => "original-b5-nearest-rounded-refused",
            Self::DeclaredAdjacentSupported => "declared-adjacent-knot3-supported",
        }
    }

    fn seconds(self) -> [f64; 5] {
        let mut seconds = ANCHORS.map(|frame| frame as f64 / f64::from(RATE));
        if matches!(self, Self::DeclaredAdjacentSupported) {
            // A separately declared stored map, not a production-boundary tolerance.
            seconds[3] = seconds[3].next_down();
        }
        seconds
    }

    fn report(self) {
        let seconds = self.seconds();
        let actual_rates = std::array::from_fn::<_, 4, _>(|index| {
            f64::from(RATE) * ((seconds[index + 1] - seconds[index]) / 2.0) / (SPAN as f64 / 2.0)
        });
        let expected_rate_bits = match self {
            Self::OriginalB5NearestRounded => [
                0x3fe0_0000_0000_0000,
                0x3ff0_0000_0000_0001,
                0x4000_0000_0000_0001,
                0x3ff8_0000_0000_0000,
            ],
            Self::DeclaredAdjacentSupported => [
                0x3fe0_0000_0000_0000,
                0x3ff0_0000_0000_0001,
                0x3fff_ffff_ffff_fffe,
                0x3ff8_0000_0000_0003,
            ],
        };
        assert_eq!(actual_rates.map(f64::to_bits), expected_rate_bits);
        let displacement = (seconds[3] - ANCHORS[3] as f64 / f64::from(RATE)) * f64::from(RATE);
        emit(
            json!({"stage":"declared_geometry_before_audio","geometry_profile":self.label(),
            "nominal_source_frames":ANCHORS,"nominal_source_rates":RATES,
            "stored_source_seconds":seconds,"stored_source_seconds_bits":seconds.map(bits),
            "actual_local_rates":actual_rates,"actual_local_rate_bits":actual_rates.map(bits),
            "knot3_displacement_loaded_frames":displacement,
            "original_exact_record_or_exact_r2_boundary_accepted":false,
            "independent_reference_uses_unchanged_nominal_equations":true,
            "coordinate_budget_frames":1.0e-8,"dry_pcm_budget":1.0e-7,
            "complete_native_pcm_budget":3.0e-6,"fitted_to_wet_output":false}),
        );
    }
}

unsafe extern "C" {
    #[link_name = "rubberband_live_new"]
    fn raw_new(rate: u32, channels: u32, options: i32) -> *mut RubberBandLiveStateOpaque;
    #[link_name = "rubberband_live_delete"]
    fn raw_delete(state: *mut RubberBandLiveStateOpaque);
    #[link_name = "rubberband_live_reset"]
    fn raw_reset(state: *mut RubberBandLiveStateOpaque);
    #[link_name = "rubberband_live_set_pitch_scale"]
    fn raw_pitch(state: *mut RubberBandLiveStateOpaque, pitch: f64);
    #[link_name = "rubberband_live_get_start_delay"]
    fn raw_delay(state: *mut RubberBandLiveStateOpaque) -> u32;
    #[link_name = "rubberband_live_get_block_size"]
    fn raw_block(state: *mut RubberBandLiveStateOpaque) -> u32;
    #[link_name = "rubberband_live_shift"]
    fn raw_shift(
        state: *mut RubberBandLiveStateOpaque,
        input: *const *const f32,
        output: *const *mut f32,
    );
}

fn emit(value: Value) {
    println!("OWNED_MAPPED_NATIVE_OUTPUT {value}");
}

fn coordinate(frame: usize) -> f64 {
    // Source-only frozen equations, never a DUT map query or read receipt.
    let frame = frame % OUTPUT;
    let segment = frame / SPAN;
    ANCHORS[segment] as f64 + (frame % SPAN) as f64 * RATES[segment]
}

fn independent_input(pcm: &[f32], frame: usize) -> [f32; 2] {
    let position = coordinate(frame);
    let left = position.floor() as usize;
    let right = (left + 1) % SOURCE;
    let fraction = position.fract() as f32;
    std::array::from_fn(|channel| {
        let a = pcm[left * 2 + channel];
        let b = pcm[right * 2 + channel];
        a + (b - a) * fraction
    })
}

fn interleaved_hash(planar: &[Vec<f32>]) -> String {
    let samples = (0..planar[0].len())
        .flat_map(|frame| [planar[0][frame], planar[1][frame]])
        .collect::<Vec<_>>();
    pcm_hash(&samples)
}

struct RawOracle {
    state: *mut RubberBandLiveStateOpaque,
    delay: usize,
    minimum_history: usize,
}

impl RawOracle {
    fn new(pitch: f64) -> Self {
        Self::with_profile(pitch, NativeOutputProfile::Short)
    }

    fn with_profile(pitch: f64, profile: NativeOutputProfile) -> Self {
        // Pinned Live C ABI/options and exact setup, independently of DUT wrappers.
        let (options, window) = match profile {
            NativeOutputProfile::Short => (0x1000_0000, 2048.0),
            NativeOutputProfile::Medium => (0x1010_0000, 2560.0),
        };
        let state = unsafe { raw_new(RATE, 2, options) };
        assert!(!state.is_null());
        assert_eq!(unsafe { raw_block(state) } as usize, BLOCK);
        unsafe {
            raw_pitch(state, if pitch > 2.0 { 1.0 } else { pitch });
            raw_reset(state);
        }
        let mut oracle = Self {
            state,
            delay: 0,
            minimum_history: 4096,
        };
        if pitch > 2.0 {
            // This unobserved initialization block is all zero input/output, not an adapter
            // block and not part of the subsequent raw stream's index zero.
            let warm = oracle.shift(&[[0.0; BLOCK]; 2]);
            assert!(warm.iter().flatten().all(|sample| *sample == 0.0));
            unsafe { raw_pitch(state, pitch) };
        }
        oracle.delay = unsafe { raw_delay(state) } as usize;
        if !(0.5..=2.0).contains(&pitch) {
            // Independent pinned 48k profile window, exact reported delay and bounds.
            oracle.minimum_history =
                ((window * pitch.max(1.0 / pitch)).ceil() as usize + oracle.delay + 2 * BLOCK)
                    .max(4096)
                    .div_ceil(BLOCK)
                    * BLOCK;
        }
        oracle
    }

    fn shift(&self, input: &[[f32; BLOCK]; 2]) -> [[f32; BLOCK]; 2] {
        let mut output = [[0.0; BLOCK]; 2];
        let input_ptrs = input.each_ref().map(|channel| channel.as_ptr());
        let output_ptrs = output.each_mut().map(|channel| channel.as_mut_ptr());
        unsafe { raw_shift(self.state, input_ptrs.as_ptr(), output_ptrs.as_ptr()) };
        assert!(output.iter().flatten().all(|sample| sample.is_finite()));
        output
    }

    fn capture(&self, pcm: &[f32], history: usize) -> Vec<Vec<f32>> {
        self.capture_with_input(pcm, history, history + OUTPUT, independent_input)
    }

    fn capture_with_input(
        &self,
        pcm: &[f32],
        silent_history: usize,
        total_input_frames: usize,
        input_at: fn(&[f32], usize) -> [f32; 2],
    ) -> Vec<Vec<f32>> {
        // Direct complete Native blocks only: no copied adapter/FIFO/source-reader routine.
        // The DUT retains any final partial input block; it has not processed those frames.
        let processed = total_input_frames / BLOCK * BLOCK;
        let mut output = (0..2)
            .map(|_| Vec::with_capacity(processed))
            .collect::<Vec<_>>();
        for first in (0..processed).step_by(BLOCK) {
            let input = std::array::from_fn(|channel| {
                std::array::from_fn(|offset| {
                    (first + offset)
                        .checked_sub(silent_history)
                        .map_or(0.0, |frame| input_at(pcm, frame)[channel])
                })
            });
            let shifted = self.shift(&input);
            for channel in 0..2 {
                output[channel].extend_from_slice(&shifted[channel]);
            }
        }
        output
    }
}

impl Drop for RawOracle {
    fn drop(&mut self) {
        unsafe { raw_delete(self.state) };
    }
}

struct OwnedBirth {
    path: LaunchPath,
    _job: OfflineAnalysisJob,
    _delivery: LoaderEventDelivery,
    captured: CapturedPadStart,
    work: Box<crate::audio_engine::ordinary_start::OrdinaryStartWork>,
    request: NativeHistoryRequest,
}

struct CapturedFixture {
    path: LaunchPath,
    job: OfflineAnalysisJob,
    delivery: LoaderEventDelivery,
    captured: CapturedPadStart,
}

fn captured_fixture(
    pcm: &[i16],
    decoded: &[f32],
    shift: i32,
    geometry: GeometryProfile,
) -> CapturedFixture {
    geometry.report();
    captured_fixture_with_geometry(pcm, decoded, shift, SOURCE, &geometry.seconds())
}

fn captured_fixture_with_geometry(
    pcm: &[i16],
    decoded: &[f32],
    shift: i32,
    source_frames: usize,
    stored_seconds: &[f64],
) -> CapturedFixture {
    assert!(stored_seconds.len() >= 2);
    let duration = source_frames as f64 / f64::from(RATE);
    let mut source = ColdCustody::new_selected_with_interleaved_signal(
        Some(crate::audio_engine::cold_residency::ResidentLoadHint {
            start_s: 0.0,
            end_s: duration,
            key_lock: false,
        }),
        pcm,
        2,
    );
    let loaded = source.engine.sample_cache.lock().unwrap()[0]
        .clone()
        .unwrap();
    assert_eq!(loaded.channels, 2);
    assert_eq!(loaded.frame_count(), source_frames);
    assert_eq!(loaded.resident_start(), 0);
    assert_eq!(loaded.resident_end(), source_frames);
    assert!(
        loaded
            .samples
            .iter()
            .zip(decoded)
            .all(|(a, b)| a.to_bits() == b.to_bits())
    );
    assert_eq!(loaded.samples.len(), decoded.len());
    source.engine.input_runtime = Some(
        crate::audio_engine::input_mapping::InputRuntime::new_with_ownership(
            source.producer.clone(),
            crate::audio_engine::timing::InputClock::new(),
            source.engine.input_runtime_ownership.clone(),
        ),
    );
    let mut path = LaunchPath::from_source(source);
    let period = SPAN as f64 / (2.0 * f64::from(RATE));
    path.source.mixer.set_master_period(period);
    assert!(path.timeline.set_master_period(period));
    let intent = PitchIntent::new(shift.clamp(-23, 23), shift - shift.clamp(-23, 23)).unwrap();
    path.source.mixer.set_pad_pitch_intent(0, intent).unwrap();

    // Reuse the actual decoded manifest. The old B5 synthetic descriptor grants no custody.
    let mut expected = record(&path.source, 0);
    expected["record"]["raw"]["detector"]["code_revision"] =
        json!("owned-mapped-native-output-authored-v1");
    expected["record"]["raw"]["uncertainty"] =
        json!("authored software-fixture coordinates; no automatic or musical qualification");
    expected["record"]["accepted"]["anchors"] = json!(
        stored_seconds
            .iter()
            .enumerate()
            .map(|(index, seconds)| json!({
                "id":format!("b5-source-{index}"),
                "source_seconds_bits":bits(*seconds),
                "beat_bits":bits(index as f64 * 2.0),
                "raw_beat_index":null,"review":"reviewed","sample_edit":null
            }))
            .collect::<Vec<_>>()
    );
    let interval = expected["record"]["accepted"]["intervals"][0].clone();
    expected["record"]["accepted"]["intervals"] = json!(vec![interval; stored_seconds.len() - 1]);
    seal(&mut expected);
    let (job, event) = stored_event(&path.source, &expected);
    let delivery = Python::attach(|py| {
        let event = event.bind(py).cast::<PyDict>().unwrap();
        let owner = event.get_item("_native_event_owner").unwrap().unwrap();
        let delivery = owner.extract::<PyRef<'_, LoaderEventDelivery>>().unwrap();
        assert!(
            job.resolve_source_beatmap_artifact_completion(&delivery)
                .unwrap()
                .is_some()
        );
        delivery.clone()
    });
    let row = Python::attach(|py| {
        let row = PyDict::new(py);
        row.set_item("sync_requested", true).unwrap();
        row.set_item("intent_revision", 1).unwrap();
        row.set_item("delivery", Py::new(py, delivery.clone()).unwrap())
            .unwrap();
        row.set_item("exact_quarters", py.None()).unwrap();
        crate::audio_engine::source_map_api::parse_rows(py, vec![row.unbind()])
            .unwrap()
            .pop()
            .unwrap()
    });
    let mut bindings = vec![None; NUM_SAMPLES];
    bindings[0] = Some(binding(&path.source, 0));
    let mut rows = vec![SourceMapRow::default(); NUM_SAMPLES];
    rows[0] = row;
    path.source
        .engine
        .input_runtime
        .as_ref()
        .unwrap()
        .set_runtime_state_with_source_maps(
            true,
            (0..NUM_SAMPLES).map(|id| id == 0).collect(),
            vec![0.0; NUM_SAMPLES],
            vec![Some(duration); NUM_SAMPLES],
            bindings.iter().map(Option::as_ref).collect(),
            Some(vec![
                crate::audio_engine::input_mapping::PitchPadProjection {
                    sync: true,
                    ..Default::default()
                };
                NUM_SAMPLES
            ]),
            Some(rows),
        )
        .unwrap();
    path.render(1);
    let captured = path
        .source
        .engine
        .capture_pad_start(0, 0.0, Some(duration), 1.0, false, None, None)
        .unwrap();
    assert!(captured.mapped_geometry().is_some());
    mapped_fixed_pitch_birth_tests::drain_grid(&mut path);
    let target = captured.target_frame().unwrap();
    assert_eq!(target, (SPAN / 2) as u64);
    captured.preparation_guard().unwrap();
    captured.callback_guard().unwrap();
    CapturedFixture {
        path,
        job,
        delivery,
        captured,
    }
}

fn owned_birth(pcm: &[i16], decoded: &[f32], shift: i32) -> OwnedBirth {
    owned_birth_from_capture(
        captured_fixture(
            pcm,
            decoded,
            shift,
            GeometryProfile::DeclaredAdjacentSupported,
        ),
        shift,
        SOURCE,
    )
}

fn owned_birth_from_capture(
    fixture: CapturedFixture,
    shift: i32,
    source_frames: usize,
) -> OwnedBirth {
    let CapturedFixture {
        path,
        job,
        delivery,
        captured,
    } = fixture;
    let target = captured.target_frame().unwrap();
    let duration = source_frames as f64 / f64::from(RATE);
    let intent = PitchIntent::new(shift.clamp(-23, 23), shift - shift.clamp(-23, 23)).unwrap();
    let mut work = captured.take_work(None).unwrap().unwrap();
    work.freeze_mapped_clock(Some(path.timeline), target)
        .unwrap();
    work.freeze_frames(target, target).unwrap();
    let (clock, target) = work.mapped_clock().unwrap();
    let mut snapshot = path
        .source
        .mixer
        .normal_start_snapshot_for_test(0, 0.0, Some(duration))
        .unwrap();
    snapshot.playback =
        SourcePlayback::mapped(captured.mapped_geometry().unwrap(), clock, target).unwrap();
    let position = snapshot.playback.position();
    snapshot.plan.frame_pos = position.frame;
    snapshot.plan.seek_mode = position.seek_mode;
    snapshot.plan.loop_period = snapshot.playback.reader_period();
    work.freeze_launch_origin(SourceReadCheckpoint {
        playback: snapshot.playback,
        plan: snapshot.plan,
    })
    .unwrap();
    let permit = work.capture_history_permit(&snapshot.sample, RATE).unwrap();
    let mut request = snapshot
        .into_request(permit, intent, false, target, work.sequence(), work.epoch())
        .unwrap();
    request.mapped_input = Some(Box::new(
        MappedNativeInput::capture(&work, &request).unwrap(),
    ));
    OwnedBirth {
        path,
        _job: job,
        _delivery: delivery,
        captured,
        work,
        request,
    }
}

#[test]
fn original_b5_rounded_geometry_keeps_genuine_mapped_rate_refusal() {
    let authored = CanonicalFixture::new(Signal::IsolatedAttack(0));
    authored.verify_pcm();
    let pcm = authored
        .sample
        .samples
        .iter()
        .map(|sample| {
            (f64::from(*sample) * 32768.0)
                .round()
                .clamp(i16::MIN as f64, i16::MAX as f64) as i16
        })
        .collect::<Vec<_>>();
    let decoded = pcm
        .iter()
        .map(|sample| f32::from(*sample) / 32768.0)
        .collect::<Vec<_>>();
    let fixture = captured_fixture(&pcm, &decoded, 7, GeometryProfile::OriginalB5NearestRounded);
    let mut work = fixture.captured.take_work(None).unwrap().unwrap();
    let target = fixture.captured.target_frame().unwrap();
    let execution = fixture.captured.execution_frame();
    let state = fixture.captured.state();
    let timeline = fixture.path.timeline;
    assert!(matches!(
        work.freeze_mapped_clock(Some(timeline), target),
        Err(PitchActionReject::MappedTimingUnavailable)
    ));
    assert!(work.mapped_clock().is_none());
    assert_eq!(fixture.captured.target_frame(), Some(target));
    assert_eq!(fixture.captured.execution_frame(), execution);
    assert_eq!(fixture.captured.state(), state);
    assert!(fixture.captured.effective_frame().is_none());
    assert_eq!(
        fixture.path.timeline.output_frame(),
        timeline.output_frame()
    );
    assert!(fixture.path.timeline.matches_clock_epoch(&timeline));
    fixture.captured.preparation_guard().unwrap();
    fixture.captured.callback_guard().unwrap();
    assert!(
        !fixture
            .path
            .source
            .mixer
            .voices
            .iter()
            .any(|voice| voice.active && voice.sample_id == 0)
    );
    assert_eq!(sync_feedback(&fixture.path).unwrap()["effective"], false);
    emit(json!({"stage":"original_geometry_genuine_refusal",
        "geometry_profile":GeometryProfile::OriginalB5NearestRounded.label(),
        "reason":"mapped_timing_unavailable","original_target":target,
        "effective_output_frame":null,"source_and_action_guards_current":true,
        "production_rate_guard_unchanged":true,"public_native_admission":false}));
}

struct Capture {
    output: Vec<Vec<f32>>,
    dry: Vec<Vec<f32>>,
    raw: Vec<Vec<f32>>,
    raw_cut: usize,
    target: u64,
    native_delay: usize,
    minimum_history: usize,
    profile: NativeOutputProfile,
}

struct CaptureProtocol {
    shift: i32,
    block_phase: usize,
    caller: (&'static str, &'static [usize]),
    profile: NativeOutputProfile,
    history: usize,
    raw_cut: usize,
    input_advance: usize,
    native_delay: usize,
    minimum_history: usize,
    coordinate: fn(usize) -> f64,
    input_at: fn(&[f32], usize) -> [f32; 2],
    geometry_label: &'static str,
}

fn capture(
    pcm: &[i16],
    decoded: &[f32],
    shift: i32,
    block_phase: usize,
    caller: (&'static str, &'static [usize]),
) -> Capture {
    capture_profile(
        pcm,
        decoded,
        shift,
        block_phase,
        caller,
        NativeOutputProfile::Short,
    )
}

fn capture_profile(
    pcm: &[i16],
    decoded: &[f32],
    shift: i32,
    block_phase: usize,
    caller: (&'static str, &'static [usize]),
    profile: NativeOutputProfile,
) -> Capture {
    assert!(matches!(block_phase, 0 | 17));
    let fixture = owned_birth(pcm, decoded, shift);
    let pitch = 2.0_f64.powf(f64::from(shift) / 12.0);
    let oracle = match profile {
        NativeOutputProfile::Short => RawOracle::new(pitch),
        NativeOutputProfile::Medium => RawOracle::with_profile(pitch, profile),
    };
    let history = oracle.minimum_history + block_phase;
    let raw_cut = history - (BLOCK - 1);
    let nominal_compensation = oracle.delay + BLOCK - 1;
    emit(json!({"stage":"declared_profile_before_source_signal",
        "profile":profile.label(),"options_bits":format!("{:08x}",profile.options().bits()),
        "shift":shift,"h":pitch,"native_block_phase":block_phase,"caller":caller.0,
        "N":oracle.delay,"Cnom":nominal_compensation,"Hmin":oracle.minimum_history,
        "H":history,"D":raw_cut,"F":history,"output_frames":OUTPUT,
        "nominal_raw_time_equation":"T+raw_index-D-Cnom",
        "negative_time_prefix_preserved":true,"fitted_to_response":false}));
    let raw = oracle.capture(decoded, history);
    assert!(raw[0].len() >= raw_cut + OUTPUT);
    // Preserve and inspect the WHOLE raw Native prefix, separately from the adapter lead.
    assert!(
        raw.iter()
            .all(|channel| channel[..raw_cut].iter().all(|sample| *sample == 0.0))
    );
    render_owned_capture(
        fixture,
        decoded,
        raw,
        CaptureProtocol {
            shift,
            block_phase,
            caller,
            profile,
            history,
            raw_cut,
            input_advance: 0,
            native_delay: oracle.delay,
            minimum_history: oracle.minimum_history,
            coordinate,
            input_at: independent_input,
            geometry_label: GeometryProfile::DeclaredAdjacentSupported.label(),
        },
    )
}

fn render_owned_capture(
    fixture: OwnedBirth,
    decoded: &[f32],
    raw: Vec<Vec<f32>>,
    protocol: CaptureProtocol,
) -> Capture {
    let CaptureProtocol {
        shift,
        block_phase,
        caller,
        profile,
        history,
        raw_cut,
        input_advance,
        native_delay,
        minimum_history,
        coordinate,
        input_at,
        geometry_label,
    } = protocol;
    let fed_frames = history + input_advance;
    assert_eq!(raw_cut + BLOCK - 1, fed_frames);
    assert!(raw.iter().all(|channel| channel.len() >= raw_cut + OUTPUT));
    let source = fixture.request.sample.clone();
    let stems = fixture.request.stems.clone();
    let accepted = fixture.request.binding.accepted;
    let intent = fixture.request.pitch_intent;
    let mut playback = fixture.request.playback;
    let mut plan = fixture.request.plan;
    let target = fixture.captured.target_frame().unwrap();
    let pitch = 2.0_f64.powf(f64::from(shift) / 12.0);
    let mut candidate = Box::new(NativeAdapterState::new(Some(profile.native()), 2));
    let native = candidate.rubberband.as_ref().unwrap().state_address();
    candidate
        .prepare_owned_mapped_birth_future(
            fixture.request,
            RetainedNativeStartCut {
                history_output_frames: history,
                discarded_raw_output_frames: raw_cut,
            },
        )
        .unwrap();
    // These cached observations were queried by exact preparation before its first
    // source block; source processing does not remeasure or replace them.
    let prepared_native = candidate.rubberband.as_ref().unwrap();
    assert_eq!(prepared_native.start_delay(), native_delay);
    assert_eq!(
        prepared_native.source_history_frames(pitch).unwrap(),
        minimum_history
    );
    let mut processor = StretchProcessor::new(2);
    processor.reset();
    assert_eq!(processor.preparation_epoch(), fixture.work.epoch());
    assert!(processor.adopt_prepared_pitch(
        &mut candidate,
        &ProductiveSourceFeed {
            pitch_intent: intent,
            key_lock: false,
            sample: &source,
            stems: stems.as_ref(),
            outgoing_stems: None,
            sample_rate_hz: RATE,
            accepted,
            plan,
            playback: &playback,
            permit: None,
            output_frame: Some(target),
        },
        fixture.work.epoch(),
        fixture.captured.sequence(),
    ));
    let mut dry_processor = StretchProcessor::new(2);
    let mut output = (0..2)
        .map(|_| Vec::with_capacity(OUTPUT))
        .collect::<Vec<_>>();
    let mut dry = (0..2)
        .map(|_| Vec::with_capacity(OUTPUT))
        .collect::<Vec<_>>();
    let mut emitted = 0;
    let mut partition = 0;
    while emitted < OUTPUT {
        let stop = (emitted + caller.1[partition % caller.1.len()]).min(OUTPUT);
        fixture.captured.preparation_guard().unwrap();
        fixture.captured.callback_guard().unwrap();
        while emitted < stop {
            playback
                .refresh_mapped_span(fixture.captured.mapped_geometry().unwrap())
                .unwrap();
            let position = playback.position();
            plan.frame_pos = position.frame;
            plan.seek_mode = position.seek_mode;
            let bound = playback.chunk(stop - emitted).0;
            let frames = processor.retained_source_chunk_limit(&playback, plan, bound);
            assert!(frames > 0);
            let before = processor
                .retained_mapped_output_coordinates()
                .unwrap()
                .unwrap();
            assert_eq!(before.logical_output_frame, target + emitted as u64);
            assert_eq!(before.raw_output_frame, (raw_cut + emitted) as u64);
            assert_eq!(
                before.actual_input_frame,
                target + (input_advance + emitted) as u64
            );
            let feed = ProductiveSourceFeed {
                pitch_intent: intent,
                key_lock: false,
                sample: &source,
                stems: stems.as_ref(),
                outgoing_stems: None,
                sample_rate_hz: RATE,
                accepted,
                plan,
                playback: &playback,
                permit: None,
                output_frame: Some(target + emitted as u64),
            };
            let dry_feed = ProductiveSourceFeed {
                pitch_intent: PitchIntent::default(),
                ..feed
            };
            let receipt = processor.process_source(feed, frames, false).unwrap();
            assert_eq!(receipt.domain, SourceReadDomain::NativeInput);
            assert_eq!(
                receipt.playback.mapped_output_frame(),
                Some(target + (input_advance + emitted) as u64)
            );
            // This separate dry renderer shares the original source/clock, not mutable wet state.
            let dry_receipt = dry_processor
                .process_source(dry_feed, frames, false)
                .unwrap();
            assert_eq!(dry_receipt.domain, SourceReadDomain::DryOutput);
            for offset in 0..=frames {
                let actual = receipt.playback.position_at(offset).unwrap();
                let dry_position = dry_receipt.playback.position_at(offset).unwrap();
                let expected_actual = coordinate(input_advance + emitted + offset);
                let expected_dry = coordinate(emitted + offset);
                assert!((actual.frame as f64 + actual.fraction - expected_actual).abs() <= 1.0e-8);
                assert!(
                    (dry_position.frame as f64 + dry_position.fraction - expected_dry).abs()
                        <= 1.0e-8
                );
            }
            for offset in 0..frames {
                let expected = input_at(decoded, emitted + offset);
                for channel in 0..2 {
                    let dry_sample = dry_processor.output_buffers()[channel][offset];
                    assert!((dry_sample - expected[channel]).abs() <= 1.0e-7);
                    dry[channel].push(dry_sample);
                    output[channel].push(processor.output_buffers()[channel][offset]);
                }
            }
            assert!(playback.advance(frames));
            emitted += frames;
            assert_eq!(processor.native_state_address(), native);
            assert_eq!(processor.native_pitch_scale_observation(), Some(pitch));
            assert_eq!(
                processor.pending_fifo_frames(),
                (
                    (fed_frames + emitted) % BLOCK,
                    BLOCK - 1 - (fed_frames + emitted) % BLOCK,
                )
            );
            assert!(processor.wet_render_ready());
        }
        partition += 1;
    }
    assert!(output.iter().flatten().all(|sample| sample.is_finite()));
    assert!(dry.iter().flatten().all(|sample| sample.is_finite()));
    let error = (0..2)
        .flat_map(|channel| {
            let output = &output[channel];
            let expected = &raw[channel][raw_cut..raw_cut + OUTPUT];
            output.iter().zip(expected).map(|(a, b)| (a - b).abs())
        })
        .fold(0.0_f32, f32::max);
    assert!(
        error <= 3.0e-6,
        "shift={shift} phase={block_phase} caller={} error={error}",
        caller.0
    );
    assert_eq!(processor.native_primed_output_frames(), fed_frames);
    assert_eq!(
        processor.productive_history().unwrap().fed_output_frames,
        (fed_frames + OUTPUT) as u64
    );
    assert!(fixture.captured.effective_frame().is_none());
    assert_eq!(sync_feedback(&fixture.path).unwrap()["effective"], false);
    emit(
        json!({"stage":"complete_stream","shift":shift,"h":pitch,"key_lock":false,
        "profile":profile.label(),
        "geometry_profile":geometry_label,
        "native_block_phase":block_phase,"creative_phase_output_frames":0,"caller":caller.0,
        "target":target,"logical_output_range":[target,target+OUTPUT as u64],
        "H":history,"D":raw_cut,"F":fed_frames,"adapter_initial_silent_frames":BLOCK-1,
        "raw_native_frames":raw[0].len(),"raw_prefix_frames":raw_cut,
        "raw_prefix_energy":raw.iter().map(|ch|ch[..raw_cut].iter().map(|x|f64::from(*x).powi(2)).sum::<f64>()).sum::<f64>(),
        "raw_native_sha256":interleaved_hash(&raw),"emitted_sha256":interleaved_hash(&output),
        "dry_sha256":interleaved_hash(&dry),"complete_pcm_error":error,"native_identity":native,
        "source_identity":fixture.captured.mapped_geometry().unwrap().map().source_identity(),
        "map_revision":fixture.captured.mapped_geometry().unwrap().map().map_revision(),
        "nominal_delay_observation_only":native_delay,"expanded_unity_warm_retarget":pitch>2.0,
        "separate_unity_warm_frames":if pitch>2.0 {BLOCK} else {0},
        "separate_unity_warm_output_all_zero":true,
        "accepted_musical_c":null,"public_native_admission":false,
        "dry_scope":"separate original-qualified renderer component, not public second-PAD adoption"}),
    );
    Capture {
        output,
        dry,
        raw,
        raw_cut,
        target,
        native_delay,
        minimum_history,
        profile,
    }
}

fn isolated_metrics(label: &str, marker: usize, capture: &Capture) {
    let energy = stereo_energy(&capture.output);
    let dry_energy = stereo_energy(&capture.dry);
    let raw_energy = stereo_energy(&capture.raw);
    let total = energy.iter().sum::<f64>();
    let raw_total = raw_energy.iter().sum::<f64>();
    if total <= 0.0 || raw_total <= 0.0 || dry_energy.iter().sum::<f64>() <= 0.0 {
        emit(json!({"stage":"isolated","label":label,"missing_response":true,"pass":false}));
        return;
    }
    let dry_q = energy_times(&dry_energy, RATE);
    let wet_q = energy_times(&energy, RATE);
    let errors = std::array::from_fn::<_, 3, _>(|index| wet_q[index] - dry_q[index]);
    let budget = f64::from(RATE) * TIMING_BUDGET_MS / 1000.0;
    let budget_frames = budget.floor() as i64;
    let timing_lower = errors[0].max(errors[1]) - budget_frames;
    let timing_upper = errors[0].min(errors[1]) + budget_frames;
    let (raw_peak_frame, raw_peak) = stereo_peak(&capture.raw);
    let (peak_frame, peak) = stereo_peak(&capture.output);
    let retained_peak = (capture.raw_cut..capture.raw_cut + OUTPUT).contains(&raw_peak_frame)
        && peak_frame + capture.raw_cut == raw_peak_frame
        && (peak - raw_peak).abs() <= 3.0e-6;
    let discarded = raw_energy[..capture.raw_cut].iter().sum::<f64>() / raw_total;
    let tail = energy[OUTPUT - RATE as usize / 50..].iter().sum::<f64>() / total;
    let extra_suffix = raw_energy[capture.raw_cut + OUTPUT..].iter().sum::<f64>() / raw_total;
    let complete = tail <= MAX_CAPTURE_TAIL_ENERGY && extra_suffix <= MAX_CAPTURE_TAIL_ENERGY;
    let retention_upper =
        raw_peak_frame.min(maximum_energy_discard(&raw_energy)) as i64 - capture.raw_cut as i64;
    let joint_upper = timing_upper.min(retention_upper);
    let timing_pass = errors[..2]
        .iter()
        .all(|error| (*error as f64).abs() <= budget);
    let pass = complete && timing_pass && retained_peak && discarded <= MAX_DISCARDED_ENERGY;
    emit(
        json!({"stage":"isolated","label":label,"declared_dry_event_frame":marker,
        "original_target":capture.target,"declared_dry_event_output_frame":capture.target+marker as u64,
        "frames":OUTPUT,"raw_frames":capture.raw[0].len(),"valid_isolation":"one source-authored event; no nonlinear subtraction",
        "dry_quantiles":dry_q,"wet_quantiles":wet_q,"natural_clock_errors_q10_q50_q90":errors,
        "timing_budget_ms":TIMING_BUDGET_MS,"timing_q10_q50_pass":timing_pass,
        "original_raw_peak_frame":raw_peak_frame,"original_raw_peak":raw_peak,
        "emitted_peak_frame":peak_frame,"emitted_peak":peak,"original_peak_retained":retained_peak,
        "discarded_complete_raw_energy_fraction":discarded,"discard_budget":MAX_DISCARDED_ENERGY,
        "final_20ms_energy_fraction":tail,"extra_raw_suffix_energy_fraction":extra_suffix,
        "tail_budget":MAX_CAPTURE_TAIL_ENERGY,"bounded_response_complete":complete,
        "timing_c_interval":[timing_lower,timing_upper],"retention_u":retention_upper,
        "joint_c_interval":[timing_lower,joint_upper],
        "timing_interval_exists":timing_lower<=timing_upper,
        "joint_interval_exists":complete && timing_lower<=joint_upper,
        "pass":pass,"accepted_musical_c":null,"fitted_delay_gain_crop":false,
        "internal_native_tail_exhaustion_proven":false,"public_or_human_acceptance":false}),
    );
}

fn tone_metrics(label: &str, shift: i32, output: &[Vec<f32>]) {
    for (segment, rate) in RATES.into_iter().enumerate() {
        let first = segment * SPAN + SPAN / 2;
        let last = first + 8192;
        for (channel, source_hz) in [160.0, 211.0].into_iter().enumerate() {
            let samples = &output[channel][first..last];
            let crossings = samples
                .windows(2)
                .enumerate()
                .filter(|(_, pair)| pair[0] <= 0.0 && pair[1] > 0.0)
                .map(|(frame, pair)| {
                    frame as f64 - f64::from(pair[0]) / f64::from(pair[1] - pair[0])
                })
                .collect::<Vec<_>>();
            let measured = (crossings.len() >= 3).then(|| {
                (crossings.len() - 1) as f64 * f64::from(RATE)
                    / (crossings.last().unwrap() - crossings[0])
            });
            let mean_energy = samples
                .iter()
                .map(|sample| f64::from(*sample).powi(2))
                .sum::<f64>()
                / samples.len() as f64;
            let expected = source_hz * 2.0_f64.powf(f64::from(shift) / 12.0) * rate;
            let cents = measured.map(|frequency| 1200.0 * (frequency / expected).log2());
            let eligible = crossings.len() > 8 && mean_energy > 0.001;
            emit(
                json!({"stage":"tone","label":label,"segment":segment,"channel":channel,
                "window":[first,last],"expected_hz":expected,"measured_hz":measured,"cents":cents,
                "positive_crossings":crossings.len(),"required_crossings_strict_greater_than":8,
                "mean_energy":mean_energy,"required_energy_strict_greater_than":0.001,
                "budget_cents_strict_less_than":15.0,"eligible":eligible,
                "pass":eligible && cents.is_some_and(|value|value.abs()<15.0),
                "fitted_delay_gain_crop":false,"public_or_human_acceptance":false}),
            );
        }
    }
}

#[test]
#[ignore = "explicit bounded original-qualified Native output measurement; no public, musical or device acceptance"]
fn measure_original_owned_mapped_native_complete_output() {
    emit(json!({"stage":"linked_native_identity",
        "identity":crate::audio_engine::pitch_runtime_tests::native_identity_tests::identify_loaded_native()
            .expect("actual linked Native DLL identity must be available")}));
    let signals = [
        Signal::StereoTone,
        Signal::IsolatedAttack(0),
        Signal::IsolatedAttack(SPAN / 4),
        Signal::IsolatedAttack(SPAN + SPAN / 4),
        Signal::IsolatedAttack(2 * SPAN + SPAN / 4),
        Signal::IsolatedAttack(3 * SPAN + SPAN / 4),
    ];
    let mut captures = 0;
    for signal in signals {
        let authored = CanonicalFixture::new(signal);
        authored.verify_pcm();
        let pcm = authored
            .sample
            .samples
            .iter()
            .map(|sample| {
                (f64::from(*sample) * 32768.0)
                    .round()
                    .clamp(i16::MIN as f64, i16::MAX as f64) as i16
            })
            .collect::<Vec<_>>();
        let decoded = pcm
            .iter()
            .map(|sample| f32::from(*sample) / 32768.0)
            .collect::<Vec<_>>();
        let quantization_error = authored
            .sample
            .samples
            .iter()
            .zip(&decoded)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0_f32, f32::max);
        assert!(quantization_error <= 1.0 / 65536.0);
        emit(
            json!({"stage":"declared_source","signal":format!("{signal:?}"),
            "source_frames":SOURCE,"source_channels":2,"output_frames":OUTPUT,
            "source_rates":RATES,"source_anchors":ANCHORS,"source_authored_pcm_sha256":authored.pcm_hash,
            "decoded_pcm16_sha256":pcm_hash(&decoded),"quantization_max_error":quantization_error,
            "source_authored_only_not_automatic_music":true,"creative_phase_output_frames":0,
            "native_block_phases":[0,17],"raw_lead_frames":0}),
        );
        for shift in [7, -23, 24] {
            for block_phase in [0, 17] {
                let mut previous: Option<Capture> = None;
                for caller in CALLERS {
                    let capture = capture(&pcm, &decoded, shift, block_phase, caller);
                    if let Some(prior) = &previous {
                        assert_eq!(capture.output, prior.output);
                        assert_eq!(capture.dry, prior.dry);
                        assert_eq!(capture.raw, prior.raw);
                    }
                    let label = format!("{signal:?}_k{shift}_blockphase{block_phase}_{}", caller.0);
                    match signal {
                        Signal::StereoTone => tone_metrics(&label, shift, &capture.output),
                        Signal::IsolatedAttack(marker) => {
                            isolated_metrics(&label, marker, &capture)
                        }
                    }
                    previous = Some(capture);
                    captures += 1;
                }
            }
        }
    }
    assert_eq!(captures, 72);
    emit(
        json!({"stage":"technical_measurement_complete","captures":captures,
        "acoustic_failures_are_results":true,"accepted_musical_c":null,
        "public_native_admission":false,"device_music_human_or_full_goal_acceptance":false}),
    );
}

mod mapped_native_profile_tests {
    include!("mapped_native_profile_tests.rs");
}

mod aligned_mapped_birth_tests {
    include!("aligned_mapped_birth_tests.rs");
}
