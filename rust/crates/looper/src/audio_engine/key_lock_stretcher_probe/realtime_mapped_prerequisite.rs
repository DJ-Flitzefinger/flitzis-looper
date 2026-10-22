//! A bounded, test-only R3 realtime consumer of the already mapped canonical PCM.
//! Technical protocol assertions are mandatory; acoustic rejection remains a result.

use super::*;

// Pinned v4.0.0 rubberband-c.h: RealTime | ThreadingNever | WindowStandard |
// PitchHighConsistency | ChannelsTogether | EngineFiner. Standard contributes zero.
const REALTIME_OPTIONS: c_int = 0x3401_0001;
const TIME_RATIO: f64 = 1.0;
const PREREQUISITE_PITCH: f64 = 2.0;
const EXPECTED_PAD: usize = 2048;
const PREREQUISITE_DELAY: usize = 1024;
const UNITY_DELAY: usize = 2048;
const TIMING_FRAMES: i64 = 88;
const PREREQUISITE_CAPTURES: usize = 4;
const UNITY_FRAMES: usize = 16_391;
const UNITY_EVENT: usize = 4096;
const MAX_PROCESS_CALLS: usize = MAX_INPUT_FRAMES.div_ceil(MAX_PROCESS_SIZE);
const MAX_RETRIEVE_CALLS: usize = MAX_CAPTURE_FRAMES;
const MAX_FINAL_DRAIN_CALLS: usize = 16;

fn pcm_hash(channels: &[Vec<f32>]) -> String {
    assert_eq!(channels.len(), PROBE_CHANNELS);
    assert!(
        channels
            .iter()
            .all(|channel| channel.len() == channels[0].len())
    );
    let mut hash = Sha256::new();
    for frame in 0..channels[0].len() {
        for channel in channels {
            hash.update(channel[frame].to_le_bytes());
        }
    }
    format!("{:x}", hash.finalize())
}

fn report_loaded_identity() {
    #[cfg(windows)]
    emit(serde_json::json!({
        "stage":"realtime_mapped_loaded_native_identity",
        "identity":crate::audio_engine::pitch_runtime_tests::native_identity_tests::identify_loaded_native()
            .expect("identify the already linked Native DLL without loading or replacing it"),
        "binding_scope":"existing verified Live import in the same linked RubberBand DLL; engine 3 is not library semver"
    }));
    #[cfg(not(windows))]
    emit(serde_json::json!({
        "stage":"realtime_mapped_loaded_native_identity","identity":null,
        "limit":"Windows loaded-DLL identity helper is unavailable on this platform"
    }));
}

/// One exclusive non-Send/non-Sync handle, constructed at its actual target pitch.
/// This off-thread probe never calls study, expected duration, maps or pitch retarget.
struct RealtimeStretcher {
    state: NonNull<StretcherStateOpaque>,
    pitch: f64,
    pad: usize,
    delay: usize,
    input_frames: usize,
    processed_frames: usize,
    final_calls: usize,
    blocks: Vec<usize>,
    retrieve_calls: usize,
    zero_availability_before_final: usize,
    _same_thread: PhantomData<Rc<()>>,
}

impl RealtimeStretcher {
    fn new(canonical_frames: usize, pitch: f64, expected_delay: usize) -> Self {
        assert!(pitch == 1.0 || pitch == PREREQUISITE_PITCH);
        assert!((1..=MAX_INPUT_FRAMES - EXPECTED_PAD).contains(&canonical_frames));
        const {
            assert!(REALTIME_OPTIONS & 1 == 1);
            assert!(REALTIME_OPTIONS & THREADING_NEVER == THREADING_NEVER);
            assert!(REALTIME_OPTIONS & CHANNELS_TOGETHER == CHANNELS_TOGETHER);
            assert!(REALTIME_OPTIONS & ENGINE_FINER == ENGINE_FINER);
            assert!(REALTIME_OPTIONS & WINDOW_SHORT == 0);
        }
        // SAFETY: All constructor scalars are validated bounded C representations.
        // The returned handle receives one exclusive RAII owner immediately.
        let state = unsafe {
            rubberband_new(
                RATE,
                u32::try_from(PROBE_CHANNELS).unwrap(),
                REALTIME_OPTIONS,
                TIME_RATIO,
                pitch,
            )
        };
        let mut owner = Self {
            state: NonNull::new(state).expect("realtime Stretcher construction failed"),
            pitch,
            pad: 0,
            delay: 0,
            input_frames: 0,
            processed_frames: 0,
            final_calls: 0,
            blocks: Vec::new(),
            retrieve_calls: 0,
            zero_availability_before_final: 0,
            _same_thread: PhantomData,
        };
        // SAFETY: Configuration and queries precede every signal submission on
        // this live, exclusively owned handle; none retains a PCM pointer.
        let (engine, channels, q, p, pad, delay) = unsafe {
            rubberband_set_debug_level(owner.state.as_ptr(), 0);
            rubberband_set_max_process_size(
                owner.state.as_ptr(),
                u32::try_from(MAX_PROCESS_SIZE).unwrap(),
            );
            (
                rubberband_get_engine_version(owner.state.as_ptr()),
                rubberband_get_channel_count(owner.state.as_ptr()),
                rubberband_get_time_ratio(owner.state.as_ptr()),
                rubberband_get_pitch_scale(owner.state.as_ptr()),
                rubberband_get_preferred_start_pad(owner.state.as_ptr()),
                rubberband_get_start_delay(owner.state.as_ptr()),
            )
        };
        owner.pad = usize::try_from(pad).unwrap();
        owner.delay = usize::try_from(delay).unwrap();
        owner.input_frames = owner.pad.checked_add(canonical_frames).unwrap();
        emit(serde_json::json!({
            "stage":"realtime_mapped_pre_signal_api",
            "requested_options_bits":REALTIME_OPTIONS,"requested_engine":3,"actual_engine":engine,
            "actual_channels":channels,"actual_time_ratio_q":q,"actual_pitch_p":p,
            "actual_preferred_start_pad_P":owner.pad,"actual_start_delay_N":owner.delay,
            "expected_P":EXPECTED_PAD,"expected_N":expected_delay,
            "canonical_frames":canonical_frames,"total_native_input_frames":owner.input_frames,
            "max_process_size":MAX_PROCESS_SIZE,"nominal_output_index":"raw_n - queried_N",
            "nominal_raw_window":[owner.delay,owner.delay+canonical_frames],
            "queried_before_signal":true,"borrowed_Live_adapter_511":false,
            "study_expected_duration_keyframes_warm_retarget":false
        }));
        assert_eq!(engine, 3, "engine number does not prove library version");
        assert_eq!(channels, PROBE_CHANNELS as u32);
        assert_eq!(q, TIME_RATIO);
        assert_eq!(p, pitch);
        assert_eq!(owner.pad, EXPECTED_PAD);
        assert_eq!(owner.delay, expected_delay);
        assert!(owner.input_frames <= MAX_INPUT_FRAMES);
        owner
    }

    fn process(&mut self, input: [&[f32]; PROBE_CHANNELS]) {
        assert_eq!(self.final_calls, 0);
        let frames = input[0].len();
        assert!((1..=MAX_PROCESS_SIZE).contains(&frames));
        assert!(self.blocks.len() < MAX_PROCESS_CALLS);
        let end = self.processed_frames.checked_add(frames).unwrap();
        assert!(end <= self.input_frames);
        let final_block = end == self.input_frames;
        let pointers = std::array::from_fn::<_, PROBE_CHANNELS, _>(|channel| {
            assert_eq!(input[channel].len(), frames);
            assert!(input[channel].iter().all(|sample| sample.is_finite()));
            input[channel].as_ptr()
        });
        // SAFETY: The two immutable bounded channel slices, pointer array and
        // exclusive live handle remain valid throughout this synchronous call.
        unsafe {
            rubberband_process(
                self.state.as_ptr(),
                pointers.as_ptr(),
                u32::try_from(frames).unwrap(),
                c_int::from(final_block),
            );
        }
        self.processed_frames = end;
        self.final_calls += usize::from(final_block);
        self.blocks.push(frames);
    }

    /// Every positive iteration retrieves at least one real frame. Both the
    /// output extent and total retrieve count have independent finite bounds.
    fn drain(&mut self, output: &mut [Vec<f32>]) -> c_int {
        let mut scratch = [[0.0_f32; MAX_PROCESS_SIZE]; PROBE_CHANNELS];
        for _ in 0..=MAX_RETRIEVE_CALLS {
            // SAFETY: The handle is live and exclusive; ThreadingNever has no worker.
            let available = unsafe { rubberband_available(self.state.as_ptr()) };
            assert!(available >= -1, "invalid Native availability {available}");
            if available <= 0 {
                if self.final_calls == 0 {
                    assert_eq!(available, 0, "Native completed before final input");
                    self.zero_availability_before_final += 1;
                }
                return available;
            }
            let available = usize::try_from(available).unwrap();
            assert!(available <= MAX_CAPTURE_FRAMES.checked_sub(output[0].len()).unwrap());
            assert!(self.retrieve_calls < MAX_RETRIEVE_CALLS);
            let frames = available.min(MAX_PROCESS_SIZE);
            let pointers = [scratch[0].as_mut_ptr(), scratch[1].as_mut_ptr()];
            // SAFETY: Both distinct writable scratch channels hold at least frames
            // samples. The request is bounded by the immediately queried availability.
            let retrieved = unsafe {
                rubberband_retrieve(
                    self.state.as_ptr(),
                    pointers.as_ptr(),
                    u32::try_from(frames).unwrap(),
                )
            };
            assert_eq!(usize::try_from(retrieved).unwrap(), frames);
            for channel in 0..PROBE_CHANNELS {
                assert!(
                    scratch[channel][..frames]
                        .iter()
                        .all(|sample| sample.is_finite())
                );
                output[channel].extend_from_slice(&scratch[channel][..frames]);
            }
            self.retrieve_calls += 1;
        }
        panic!("realtime capture exceeded its finite retrieval bound");
    }
}

impl Drop for RealtimeStretcher {
    fn drop(&mut self) {
        // SAFETY: This non-Clone, non-Send owner retires its one handle exactly
        // once after all synchronous calls, on its original test thread.
        unsafe { rubberband_delete(self.state.as_ptr()) };
    }
}

struct Capture {
    raw: Vec<Vec<f32>>,
    delay: usize,
}

fn padded_sample(input: &[Vec<f32>], pad: usize, channel: usize, frame: usize) -> f32 {
    frame
        .checked_sub(pad)
        .map_or(0.0, |index| input[channel][index])
}

fn capture(input: &[Vec<f32>], pitch: f64, caller: (&str, &[usize])) -> Capture {
    assert_eq!(input.len(), PROBE_CHANNELS);
    let frames = input[0].len();
    assert!(input.iter().all(|channel| channel.len() == frames));
    assert!(input.iter().flatten().all(|sample| sample.is_finite()));
    assert!(!caller.1.is_empty());
    assert!(
        caller
            .1
            .iter()
            .all(|count| (1..=MAX_PROCESS_SIZE).contains(count))
    );
    let expected_delay = if pitch == 1.0 {
        UNITY_DELAY
    } else {
        PREREQUISITE_DELAY
    };
    let mut owner = RealtimeStretcher::new(frames, pitch, expected_delay);
    let mut output = (0..PROBE_CHANNELS).map(|_| Vec::new()).collect::<Vec<_>>();
    let mut staging = [[0.0_f32; MAX_PROCESS_SIZE]; PROBE_CHANNELS];
    let mut staged = 0;
    let mut cursor = 0;
    let mut native_cursor = 0;
    let mut caller_calls = 0;
    let mut available = 0;
    while cursor < owner.input_frames {
        assert!(caller_calls < MAX_INPUT_FRAMES);
        let end = cursor + caller.1[caller_calls % caller.1.len()].min(owner.input_frames - cursor);
        while cursor < end {
            let count = (MAX_PROCESS_SIZE - staged).min(end - cursor);
            for (channel, staged_channel) in staging.iter_mut().enumerate() {
                for offset in 0..count {
                    staged_channel[staged + offset] =
                        padded_sample(input, owner.pad, channel, cursor + offset);
                }
            }
            cursor += count;
            staged += count;
            if staged == MAX_PROCESS_SIZE || cursor == owner.input_frames {
                for (channel, staged_channel) in staging.iter().enumerate() {
                    for (offset, sample) in staged_channel[..staged].iter().enumerate() {
                        assert_eq!(
                            sample.to_bits(),
                            padded_sample(input, owner.pad, channel, native_cursor + offset)
                                .to_bits()
                        );
                    }
                }
                owner.process(std::array::from_fn(|channel| &staging[channel][..staged]));
                native_cursor += staged;
                staged = 0;
                available = owner.drain(&mut output);
            }
        }
        caller_calls += 1;
    }
    assert_eq!(staged, 0);
    assert_eq!(native_cursor, owner.input_frames);
    assert_eq!(owner.processed_frames, owner.pad + frames);
    assert_eq!(owner.final_calls, 1);
    let mut expected_blocks = vec![MAX_PROCESS_SIZE; owner.input_frames / MAX_PROCESS_SIZE];
    if !owner.input_frames.is_multiple_of(MAX_PROCESS_SIZE) {
        expected_blocks.push(owner.input_frames % MAX_PROCESS_SIZE);
    }
    assert_eq!(owner.blocks, expected_blocks);
    let mut final_drain_calls = 0;
    while available != -1 && final_drain_calls < MAX_FINAL_DRAIN_CALLS {
        available = owner.drain(&mut output);
        final_drain_calls += 1;
    }
    assert_eq!(
        available, -1,
        "complete RT capture must reach actual terminal availability"
    );
    assert_eq!(output[0].len(), output[1].len());
    assert!(output[0].len() <= MAX_CAPTURE_FRAMES);
    // SAFETY: Final queries inspect the still-live exclusive owner, after all input.
    unsafe {
        assert_eq!(rubberband_get_time_ratio(owner.state.as_ptr()), TIME_RATIO);
        assert_eq!(
            rubberband_get_pitch_scale(owner.state.as_ptr()),
            owner.pitch
        );
    }
    emit(serde_json::json!({
        "stage":"realtime_mapped_complete_raw_capture","caller":caller.0,
        "pitch_p":pitch,"canonical_input_frames":frames,"queried_P":owner.pad,"queried_N":owner.delay,
        "canonical_pcm_sha256":pcm_hash(input),"complete_raw_pcm_sha256":pcm_hash(&output),
        "complete_raw_frames":output[0].len(),"actual_terminal_available":available,
        "processed_pad_plus_canonical_frames":owner.processed_frames,"canonical_samples_fed_exactly_once":true,
        "native_process_block_sequence":owner.blocks,"native_process_calls":owner.blocks.len(),
        "caller_calls":caller_calls,"final_process_calls":owner.final_calls,
        "retrieve_calls":owner.retrieve_calls,"final_drain_calls":final_drain_calls,
        "zero_availability_before_final_not_EOF":owner.zero_availability_before_final,
        "limits":{"input_frames":MAX_INPUT_FRAMES,"raw_frames":MAX_CAPTURE_FRAMES,
            "process_calls":MAX_PROCESS_CALLS,"retrieve_calls":MAX_RETRIEVE_CALLS,"final_drain_calls":MAX_FINAL_DRAIN_CALLS},
        "raw_output_cropped_fabricated_or_gain_changed":false,"callback_or_deadline_proof":false
    }));
    Capture {
        raw: output,
        delay: owner.delay,
    }
}

struct NominalMetrics {
    pass: bool,
    peak_offset: Option<i64>,
}

fn nominal_metrics(
    label: &str,
    capture: &Capture,
    dry: &[Vec<f32>],
    event: usize,
) -> NominalMetrics {
    let raw = &capture.raw;
    let frames = dry[0].len();
    assert!(event < frames);
    let start = capture.delay;
    let end = start.checked_add(frames).unwrap();
    let complete = raw[0].len() >= end;
    let energy = stereo_energy(raw);
    let total = energy.iter().sum::<f64>();
    assert!(total.is_finite());
    let response = (total > 0.0 && !raw[0].is_empty()).then(|| stereo_response(raw));
    let full_raw_times = (total > 0.0).then(|| energy_times(&energy, RATE));
    let fraction = |range: std::ops::Range<usize>| {
        (total > 0.0).then(|| energy[range].iter().sum::<f64>() / total)
    };
    let negative_prefix = fraction(0..start.min(energy.len()));
    let extra = if complete {
        fraction(end..energy.len())
    } else {
        None
    };
    let discarded = negative_prefix
        .zip(extra)
        .map(|(prefix, suffix)| prefix + suffix);
    let tail_frames = RATE as usize / 50;
    assert!(frames >= tail_frames);
    let fixed_tail = if complete {
        fraction(end - tail_frames..end)
    } else {
        None
    };
    let drained_tail = if energy.len() >= tail_frames {
        fraction(energy.len() - tail_frames..energy.len())
    } else {
        None
    };
    let dry_times = energy_times(&stereo_energy(dry), RATE);
    let nominal = complete.then(|| {
        raw.iter()
            .map(|channel| channel[start..end].to_vec())
            .collect::<Vec<_>>()
    });
    let nominal_energy = nominal.as_ref().map(|samples| stereo_energy(samples));
    let fixed_tail_nominal = nominal_energy.as_ref().and_then(|energy| {
        let total = energy.iter().sum::<f64>();
        (total > 0.0).then(|| energy[frames - tail_frames..].iter().sum::<f64>() / total)
    });
    let nominal_times = nominal_energy
        .as_ref()
        .filter(|energy| energy.iter().sum::<f64>() > 0.0)
        .map(|energy| energy_times(energy, RATE));
    let errors =
        nominal_times.map(|times| std::array::from_fn::<_, 3, _>(|i| times[i] - dry_times[i]));
    let timing =
        errors.is_some_and(|errors| errors[..2].iter().all(|error| error.abs() <= TIMING_FRAMES));
    let peak_retained = complete
        && response
            .as_ref()
            .is_some_and(|response| (start..end).contains(&response.peak_frame));
    let loss = discarded.is_some_and(|value| value <= MAX_DISCARDED_ENERGY);
    let tail = fixed_tail.is_some_and(|value| value <= MAX_CAPTURE_TAIL_ENERGY)
        && fixed_tail_nominal.is_some_and(|value| value <= MAX_CAPTURE_TAIL_ENERGY);
    let extras = extra.is_some_and(|value| value <= MAX_CAPTURE_TAIL_ENERGY);
    let pass = complete && timing && peak_retained && loss && tail && extras;
    let peak_offset = response
        .as_ref()
        .map(|response| response.peak_frame as i64 - start as i64 - event as i64);
    let mut record = serde_json::json!({
        "stage":"realtime_mapped_actual_nominal_metrics","label":label,
        "declared_raw_to_nominal_translation_N":start,"nominal_raw_window":[start,end],
        "nominal_frames":frames,"canonical_event_frame":event,"complete_raw_frames":raw[0].len(),
        "entire_fixed_nominal_window_available":complete,
        "ORIGINAL_total_raw_energy_denominator":total,"ORIGINAL_raw_peak_frame":response.as_ref().map(|response|response.peak_frame),
        "ORIGINAL_raw_peak_amplitude":response.as_ref().map(|response|response.peak),
        "ORIGINAL_raw_peak_nominal_offset_from_event":peak_offset,
        "full_raw_q10_q50_q90_in_nominal_coordinates":full_raw_times.map(|times|times.map(|time|time-start as i64)),
        "actual_nominal_q10_q50_q90":nominal_times,"dry_canonical_q10_q50_q90":dry_times,"actual_nominal_q_errors":errors,
        "negative_nominal_prefix_frames_retained_in_raw":start.min(raw[0].len()),
        "negative_nominal_prefix_ORIGINAL_energy_fraction":negative_prefix,
        "extra_raw_suffix_frames":complete.then(||raw[0].len()-end),
        "extra_raw_suffix_ORIGINAL_energy_fraction":extra,"all_removed_raw_ORIGINAL_energy_fraction":discarded,
        "fixed_nominal_final20ms_ORIGINAL_energy_fraction":fixed_tail,
        "fixed_nominal_final20ms_actual_nominal_energy_fraction":fixed_tail_nominal,
        "actual_drained_final20ms_ORIGINAL_energy_fraction":drained_tail
    });
    record.as_object_mut().unwrap().extend(serde_json::json!({
        "fixed_gates":{"timing_budget_ms":TIMING_BUDGET_MS,"timing_budget_frames":TIMING_FRAMES,
            "maximum_discarded_ORIGINAL_energy":MAX_DISCARDED_ENERGY,"maximum_fixed20ms_and_extra_energy":MAX_CAPTURE_TAIL_ENERGY,
            "actual_nominal_timing_pass":timing,"ORIGINAL_peak_retained_pass":peak_retained,
            "discarded_energy_pass":loss,"fixed_nominal_tail_pass":tail,"extra_suffix_pass":extras,
            "complete_actual_nominal_criterion_pass":pass},
        "timing_axes_or_criteria_fitted_after_measurement":false,"product_or_listening_acceptance":false
    }).as_object().unwrap().clone());
    emit(record);
    NominalMetrics { pass, peak_offset }
}

fn strict_prerequisite(label: &str, capture: &Capture, dry: &[Vec<f32>], event: usize) -> bool {
    let frames = capture.raw[0].len();
    let declared_event = capture.delay + event;
    let mut reference = vec![vec![0.0_f32; frames]; PROBE_CHANNELS];
    let copy_frames = frames.saturating_sub(capture.delay).min(dry[0].len());
    if copy_frames > 0 {
        for channel in 0..PROBE_CHANNELS {
            reference[channel][capture.delay..capture.delay + copy_frames]
                .copy_from_slice(&dry[channel][..copy_frames]);
        }
    }
    let can_diagnose = frames > RATE as usize / 50
        && declared_event < frames
        && stereo_energy(&reference).iter().sum::<f64>() > 0.0;
    let result = can_diagnose.then(|| diagnose(label, &capture.raw, &reference, declared_event));
    let pass = result.as_ref().is_some_and(|result| result.strict_pass);
    emit(serde_json::json!({
        "stage":"realtime_mapped_unchanged_strict_prerequisite","label":label,
        "dry_reference_frames_exactly_raw_frames":frames,"dry_reference_leading_zeros_N":capture.delay,
        "declared_event_N_plus_canonical_event":declared_event,"dry_canonical_frames_copied":copy_frames,
        "entire_canonical_reference_fits":copy_frames==dry[0].len(),"unchanged_diagnose_invoked":can_diagnose,
        "unchanged_strict_prerequisite_pass":pass,
        "unchanged_timing_intersection":result.as_ref().and_then(|result|result.timing_interval),
        "diagnostic_translation_never_selects_compensation":true
    }));
    pass
}

fn unity_control() -> bool {
    emit(serde_json::json!({
        "stage":"realtime_mapped_predeclared_unity_control","frames":UNITY_FRAMES,
        "event":UNITY_EVENT,"stereo_impulse":[1.0,-0.73],"time_ratio_q":TIME_RATIO,"pitch_p":1.0,
        "expected_P":EXPECTED_PAD,"expected_N":UNITY_DELAY,"nominal_axis":"raw_n-N",
        "nominal_peak_offset_budget_frames":TIMING_FRAMES,"callers":["512","irregular"],
        "PCM_error_budget":null,"PCM_error_scope":"observed only; API delay alignment does not promise sample-exact unity waveform"
    }));
    let mut input = vec![vec![0.0_f32; UNITY_FRAMES]; PROBE_CHANNELS];
    input[0][UNITY_EVENT] = 1.0;
    input[1][UNITY_EVENT] = -0.73;
    assert_eq!(stereo_peak(&input), (UNITY_EVENT, 1.0));
    assert_eq!(
        input
            .iter()
            .flatten()
            .filter(|sample| **sample != 0.0)
            .count(),
        2
    );
    let mut previous: Option<Vec<Vec<f32>>> = None;
    let mut passes = 0;
    for caller in PROCESS_PATTERNS {
        let capture = capture(&input, 1.0, caller);
        let label = format!("unity_control_{}", caller.0);
        let metrics = nominal_metrics(&label, &capture, &input, UNITY_EVENT);
        let complete = capture.raw[0].len() >= capture.delay + UNITY_FRAMES;
        let mut max_error = 0.0_f64;
        let mut squared_error = 0.0;
        if complete {
            for (channel, dry) in input.iter().enumerate() {
                for (wet, dry) in capture.raw[channel][capture.delay..capture.delay + UNITY_FRAMES]
                    .iter()
                    .zip(dry)
                {
                    let error = f64::from(*wet) - f64::from(*dry);
                    max_error = max_error.max(error.abs());
                    squared_error += error * error;
                }
            }
        }
        let peak_pass = metrics
            .peak_offset
            .is_some_and(|offset| offset.abs() <= TIMING_FRAMES);
        let control_pass = metrics.pass && peak_pass;
        passes += usize::from(control_pass);
        emit(serde_json::json!({
            "stage":"realtime_mapped_unity_control_result","caller":caller.0,
            "nominal_peak_offset_frames":metrics.peak_offset,"nominal_peak_offset_budget_frames":TIMING_FRAMES,
            "maximum_absolute_PCM_error":complete.then_some(max_error),
            "RMS_PCM_error":complete.then(||(squared_error/(PROBE_CHANNELS*UNITY_FRAMES) as f64).sqrt()),
            "PCM_error_budget":null,"sample_exact_unity_waveform_required":false,
            "fixed_nominal_coordinate_and_energy_budgets_pass":control_pass
        }));
        if let Some(reference) = &previous {
            let (equal, mismatches, error) =
                compare_complete_pcm(3, &label, reference, &capture.raw, reference[0].len());
            assert!(
                equal,
                "unity caller changed complete PCM: {mismatches} samples, max {error}"
            );
        } else {
            previous = Some(capture.raw);
        }
    }
    let pass = passes == PROCESS_PATTERNS.len();
    emit(
        serde_json::json!({"stage":"realtime_mapped_unity_control_complete","passes":passes,"captures":PROCESS_PATTERNS.len(),"pass":pass}),
    );
    pass
}

#[test]
fn realtime_standard_hc_unity_padding_control() {
    report_loaded_identity();
    assert!(
        unity_control(),
        "unity coordinate control failed; do not fit N or interpret kick mapping"
    );
}

#[test]
#[ignore = "explicit four-capture canonical kick measurement; acoustic rejection is evidence, not productive acceptance"]
fn realtime_standard_hc_canonical_kick_prerequisite() {
    assert_eq!(TIMING_BUDGET_MS, 2.0);
    assert_eq!(
        (f64::from(RATE) * TIMING_BUDGET_MS / 1000.0).floor() as i64,
        TIMING_FRAMES
    );
    assert_eq!(MAX_DISCARDED_ENERGY, 0.001);
    assert_eq!(MAX_CAPTURE_TAIL_ENERGY, 0.000_001);
    assert_eq!(
        PREREQUISITE_PITCH,
        2.0_f64.powf(f64::from(TOTAL_SEMITONES) / 12.0) / SOURCE_RATE
    );
    let source = original_source();
    report_loaded_identity();
    emit(serde_json::json!({
        "stage":"realtime_mapped_predeclared_prerequisite_matrix",
        "source_pcm_sha256":original_hash(&source),"original_source_frames":ORIGINAL_SOURCE_FRAMES,
        "sample_rate_hz":RATE,"r":SOURCE_RATE,"h":1.0,"key_lock":true,
        "canonical_warp_already_applied":true,"q":TIME_RATIO,"p":PREREQUISITE_PITCH,
        "options_bits":REALTIME_OPTIONS,"expected_P":EXPECTED_PAD,"expected_N":PREREQUISITE_DELAY,
        "source_events":[0,INTERIOR_SOURCE_EVENT],"canonical_events":[0,HISTORY],
        "canonical_frames":[262148,393220],"caller_partitions":PROCESS_PATTERNS.map(|(name,pattern)|serde_json::json!({"name":name,"frames":pattern})),"captures":PREREQUISITE_CAPTURES,
        "nominal_axis":"raw_n-N, with complete negative nominal prefix retained in raw",
        "nominal_tail_frames":RATE as usize/50,"timing_budget_frames":TIMING_FRAMES,
        "maximum_discarded_energy":MAX_DISCARDED_ENERGY,"maximum_tail_and_extra_energy":MAX_CAPTURE_TAIL_ENERGY,
        "candidate_survival":"all four actual nominal and unchanged strict prerequisites plus both unity controls",
        "no_productive_backend_or_guard_change":true
    }));
    let unity_pass = unity_control();
    let mut captures = 0;
    let mut nominal_passes = 0;
    let mut strict_passes = 0;
    let mut joint_passes = 0;
    for source_event in [0, INTERIOR_SOURCE_EVENT] {
        let source_frames = source_event + ORIGINAL_SOURCE_FRAMES;
        let canonical_frames = source_frames * 2;
        let event = source_event * 2;
        assert_eq!(
            canonical_frames,
            if source_event == 0 { 262148 } else { 393220 }
        );
        assert_eq!(event, if source_event == 0 { 0 } else { 131072 });
        let shifted = (0..PROBE_CHANNELS)
            .map(|channel| {
                (0..source_frames)
                    .map(|frame| {
                        frame
                            .checked_sub(source_event)
                            .map_or(0.0, |frame| source[channel][frame])
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let canonical = dry_mapping(&shifted, canonical_frames, source_event);
        for channel in 0..PROBE_CHANNELS {
            assert_eq!(
                canonical[channel][event].to_bits(),
                source[channel][0].to_bits()
            );
            assert!(
                canonical[channel][..event]
                    .iter()
                    .all(|sample| *sample == 0.0)
            );
        }
        if source_event == INTERIOR_SOURCE_EVENT {
            let live = live_hard_negative_input(&source);
            assert_eq!(LIVE_CAPTURE, 196608);
            for channel in 0..PROBE_CHANNELS {
                assert!(
                    canonical[channel][..LIVE_CAPTURE]
                        .iter()
                        .zip(&live[channel])
                        .all(|(a, b)| a.to_bits() == b.to_bits())
                );
            }
        }
        let mut previous: Option<Vec<Vec<f32>>> = None;
        for caller in PROCESS_PATTERNS {
            captures += 1;
            assert!(captures <= PREREQUISITE_CAPTURES);
            let capture = capture(&canonical, PREREQUISITE_PITCH, caller);
            let label = format!(
                "R3_realtime_Standard_HC_canonical_event{event}_{}",
                caller.0
            );
            let nominal = nominal_metrics(&label, &capture, &canonical, event).pass;
            let strict = strict_prerequisite(&label, &capture, &canonical, event);
            nominal_passes += usize::from(nominal);
            strict_passes += usize::from(strict);
            joint_passes += usize::from(nominal && strict);
            if let Some(reference) = &previous {
                let (equal, mismatches, error) =
                    compare_complete_pcm(3, &label, reference, &capture.raw, reference[0].len());
                assert!(
                    equal,
                    "canonical caller changed complete PCM: {mismatches} samples, max {error}"
                );
            } else {
                previous = Some(capture.raw);
            }
        }
    }
    assert_eq!(captures, PREREQUISITE_CAPTURES);
    let survived = unity_pass && joint_passes == PREREQUISITE_CAPTURES;
    emit(serde_json::json!({
        "stage":"realtime_mapped_prerequisite_complete","captures":captures,"unity_control_pass":unity_pass,
        "actual_nominal_passes":nominal_passes,"unchanged_strict_passes":strict_passes,"joint_passes":joint_passes,
        "candidate_survived_prerequisite":survived,
        "candidate_status":if !unity_pass {"BLOCKED_UNITY_CONTROL"} else if survived {"SURVIVED_PREREQUISITE_ONLY"} else {"REJECTED"},
        "acoustic_failures_remain_real_results":true,"fitted_delay_or_criteria_relaxation":false,
        "full_domain_tones_dynamic_pitch_ownership_callback_deadline_musical_or_Human_acceptance":false,
        "productive_backend_profile_or_guard_change":false
    }));
}
