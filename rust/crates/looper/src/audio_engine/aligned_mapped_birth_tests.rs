// A bounded actual-output candidate, using the existing original-owned future feed.
// The independently named fifth span supplies real silent PCM, not virtual EOF input.

use super::*;

const EXTENDED_SOURCE: usize = SOURCE + 49_152;
const EXTENDED_LOOP_OUTPUT: usize = OUTPUT + SPAN;
const ALIGNED_SHIFT: i32 = 7;
const EXPECTED_DELAY: usize = 3078;
const EXPECTED_COMPENSATION: usize = EXPECTED_DELAY + BLOCK - 1;
const GEOMETRY_LABEL: &str = "adjacent-supported-five-span-original-with-authored-silent-fifth";

fn aligned_emit(value: Value) {
    println!("ALIGNED_MAPPED_NATIVE_BIRTH {value}");
}

fn five_span_coordinate(frame: usize) -> f64 {
    assert!(frame <= EXTENDED_LOOP_OUTPUT);
    if frame < OUTPUT {
        // The original independent equations remain unchanged before their old wrap.
        coordinate(frame)
    } else {
        SOURCE as f64 + (frame - OUTPUT) as f64 * 1.5
    }
}

fn five_span_input(pcm: &[f32], frame: usize) -> [f32; 2] {
    assert_eq!(pcm.len(), EXTENDED_SOURCE * 2);
    let position = five_span_coordinate(frame);
    let left = position.floor() as usize;
    let right = left + 1;
    // Every used read has both taps in this actual original; no wrap or EOF substitute.
    assert!(right < EXTENDED_SOURCE);
    let fraction = position.fract() as f32;
    std::array::from_fn(|channel| {
        let a = pcm[left * 2 + channel];
        let b = pcm[right * 2 + channel];
        a + (b - a) * fraction
    })
}

fn five_span_seconds() -> Vec<f64> {
    let mut seconds = GeometryProfile::DeclaredAdjacentSupported
        .seconds()
        .to_vec();
    seconds.push(EXTENDED_SOURCE as f64 / f64::from(RATE));
    seconds
}

struct FiveSpanOriginal {
    pcm: Vec<i16>,
    decoded: Vec<f32>,
}

fn five_span_original(marker: usize) -> FiveSpanOriginal {
    assert!(matches!(marker, 0 | 73728));
    let authored = CanonicalFixture::new(Signal::IsolatedAttack(marker));
    authored.verify_pcm();
    let original_pcm = authored
        .sample
        .samples
        .iter()
        .map(|sample| {
            (f64::from(*sample) * 32768.0)
                .round()
                .clamp(i16::MIN as f64, i16::MAX as f64) as i16
        })
        .collect::<Vec<_>>();
    let original_decoded = original_pcm
        .iter()
        .map(|sample| f32::from(*sample) / 32768.0)
        .collect::<Vec<_>>();
    let quantization_error = authored
        .sample
        .samples
        .iter()
        .zip(&original_decoded)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0_f32, f32::max);
    assert!(quantization_error <= 1.0 / 65536.0);
    let mut pcm = original_pcm.clone();
    pcm.resize(EXTENDED_SOURCE * 2, 0);
    let decoded = pcm
        .iter()
        .map(|sample| f32::from(*sample) / 32768.0)
        .collect::<Vec<_>>();
    assert_eq!(&pcm[..SOURCE * 2], original_pcm);
    assert_eq!(&decoded[..SOURCE * 2], original_decoded);
    assert!(decoded[SOURCE * 2..].iter().all(|sample| *sample == 0.0));
    let seconds = five_span_seconds();
    assert_eq!(
        &seconds[..5],
        GeometryProfile::DeclaredAdjacentSupported.seconds()
    );
    let actual_rates = seconds
        .windows(2)
        .map(|pair| f64::from(RATE) * (pair[1] - pair[0]) / SPAN as f64)
        .collect::<Vec<_>>();
    assert!((actual_rates[4] - 1.5).abs() < 1.0e-12);
    aligned_emit(
        json!({"stage":"declared_original_before_signal","geometry_profile":GEOMETRY_LABEL,
        "marker":marker,"original_source_frames":SOURCE,"actual_source_frames":EXTENDED_SOURCE,
        "appended_real_silent_source_frames":49152,"output_capture_frames":OUTPUT,
        "new_loop_output_frames":EXTENDED_LOOP_OUTPUT,"source_anchors":[0,16384,49152,114688,163840,212992],
        "quarter_anchors":[0,2,4,6,8,10],"stored_source_seconds_bits":seconds.iter().copied().map(bits).collect::<Vec<_>>(),
        "actual_rate_bits":actual_rates.iter().copied().map(bits).collect::<Vec<_>>(),
        "source_authored_pcm_sha256":authored.pcm_hash,"original_pcm16_sha256":pcm_hash(&original_decoded),
        "new_original_pcm16_sha256":pcm_hash(&decoded),"quantization_max_error":quantization_error,
        "old_source_prefix_exact":true,"fifth_span_actual_pcm_all_zero":true,
        "logical_old_endpoint_source_frame":five_span_coordinate(OUTPUT),"old_wrapped_endpoint_source_frame":coordinate(OUTPUT),
        "exact_quarters":null,"automatic_musical_or_meter_qualification":false}),
    );
    FiveSpanOriginal { pcm, decoded }
}

struct AlignedCapture {
    capture: Capture,
    original_raw: Vec<Vec<f32>>,
}

fn capture_aligned(
    original: &FiveSpanOriginal,
    block_phase: usize,
    caller: (&'static str, &'static [usize]),
) -> AlignedCapture {
    assert!(matches!(block_phase, 0 | 17));
    let fixture = owned_birth_from_capture(
        captured_fixture_with_geometry(
            &original.pcm,
            &original.decoded,
            ALIGNED_SHIFT,
            EXTENDED_SOURCE,
            &five_span_seconds(),
        ),
        ALIGNED_SHIFT,
        EXTENDED_SOURCE,
    );
    let pitch = 2.0_f64.powf(f64::from(ALIGNED_SHIFT) / 12.0);
    let oracle = RawOracle::with_profile(pitch, NativeOutputProfile::Medium);
    let original_oracle = RawOracle::with_profile(pitch, NativeOutputProfile::Medium);
    assert_eq!(oracle.delay, EXPECTED_DELAY);
    assert_eq!(original_oracle.delay, oracle.delay);
    assert_eq!(oracle.minimum_history, 4096);
    assert_eq!(original_oracle.minimum_history, oracle.minimum_history);
    let compensation = oracle.delay + BLOCK - 1;
    assert_eq!(compensation, EXPECTED_COMPENSATION);
    let history = oracle.minimum_history + block_phase;
    let fed_frames = history + compensation;
    let raw_cut = history - (BLOCK - 1) + compensation;
    assert_eq!(fed_frames, raw_cut + BLOCK - 1);
    assert!(OUTPUT + compensation < EXTENDED_LOOP_OUTPUT);
    aligned_emit(
        json!({"stage":"declared_actual_stream_before_signal","profile":NativeOutputProfile::Medium.label(),
        "shift":ALIGNED_SHIFT,"native_block_phase":block_phase,"caller":caller.0,
        "N":oracle.delay,"Cnom":compensation,"H":history,"F":fed_frames,"Dnew":raw_cut,
        "raw_input_protocol":"H silent frames, then genuine five-span canonical source input",
        "actual_source_input_horizon_exclusive":OUTPUT+compensation,"loop_repeat_frame":EXTENDED_LOOP_OUTPUT,
        "raw_processed_frames":(fed_frames+OUTPUT)/BLOCK*BLOCK,"output_capture_frames":OUTPUT,
        "timing_budget_frames":96,"discard_budget":MAX_DISCARDED_ENERGY,"tail_budget":MAX_CAPTURE_TAIL_ENERGY,
        "fitted_offset_gain_crop":false}),
    );
    for frame in OUTPUT..OUTPUT + compensation {
        assert_eq!(five_span_input(&original.decoded, frame), [0.0; 2]);
    }
    // F controls total feed; H alone controls silence. Calling capture(..., F)
    // would replace the genuine source lookahead with silence and test a different stream.
    let raw = oracle.capture_with_input(
        &original.decoded,
        history,
        fed_frames + OUTPUT,
        five_span_input,
    );
    let original_raw = original_oracle.capture(&original.decoded[..SOURCE * 2], history);
    assert_eq!(raw[0].len(), 138752);
    for channel in 0..2 {
        assert!(raw[channel].len() >= original_raw[channel].len());
        assert!(
            raw[channel][..original_raw[channel].len()]
                .iter()
                .zip(&original_raw[channel])
                .all(|(a, b)| a.to_bits() == b.to_bits())
        );
    }
    assert_eq!(stereo_peak(&raw), stereo_peak(&original_raw));
    let capture = render_owned_capture(
        fixture,
        &original.decoded,
        raw,
        CaptureProtocol {
            shift: ALIGNED_SHIFT,
            block_phase,
            caller,
            profile: NativeOutputProfile::Medium,
            history,
            raw_cut,
            input_advance: compensation,
            native_delay: oracle.delay,
            minimum_history: oracle.minimum_history,
            coordinate: five_span_coordinate,
            input_at: five_span_input,
            geometry_label: GEOMETRY_LABEL,
        },
    );
    AlignedCapture {
        capture,
        original_raw,
    }
}

fn aligned_metrics(label: &str, marker: usize, aligned: &AlignedCapture) -> bool {
    let capture = &aligned.capture;
    // Preserve the shared fixed criteria, now on actual returned compensated PCM.
    isolated_metrics(label, marker, capture);
    let energy = stereo_energy(&capture.output);
    let dry_energy = stereo_energy(&capture.dry);
    let raw_energy = stereo_energy(&capture.raw);
    let original_raw_energy = stereo_energy(&aligned.original_raw);
    let total = energy.iter().sum::<f64>();
    let raw_total = raw_energy.iter().sum::<f64>();
    let original_raw_total = original_raw_energy.iter().sum::<f64>();
    assert!(total > 0.0 && raw_total > 0.0 && original_raw_total > 0.0);
    assert!(dry_energy.iter().sum::<f64>() > 0.0);
    assert_eq!(capture.output[0].len(), OUTPUT);
    assert_eq!(capture.output[1].len(), OUTPUT);
    let dry_q = energy_times(&dry_energy, RATE);
    let wet_q = energy_times(&energy, RATE);
    let errors = std::array::from_fn::<_, 3, _>(|index| wet_q[index] - dry_q[index]);
    let timing_pass = errors[..2].iter().all(|error| error.abs() <= 96);
    let (raw_peak_frame, raw_peak) = stereo_peak(&capture.raw);
    let (peak_frame, peak) = stereo_peak(&capture.output);
    let retained_peak = (capture.raw_cut..capture.raw_cut + OUTPUT).contains(&raw_peak_frame)
        && peak_frame + capture.raw_cut == raw_peak_frame
        && (peak - raw_peak).abs() <= 3.0e-6;
    let discarded_energy = raw_energy[..capture.raw_cut].iter().sum::<f64>();
    let discarded = discarded_energy / raw_total;
    let conservative_discarded = discarded_energy / original_raw_total;
    let tail = energy[OUTPUT - RATE as usize / 50..].iter().sum::<f64>() / total;
    let extra_suffix_energy = raw_energy[capture.raw_cut + OUTPUT..].iter().sum::<f64>();
    let extra_suffix = extra_suffix_energy / raw_total;
    let conservative_extra_suffix = extra_suffix_energy / original_raw_total;
    let added_suffix_energy = raw_energy[original_raw_energy.len()..].iter().sum::<f64>();
    let complete = tail <= MAX_CAPTURE_TAIL_ENERGY && extra_suffix <= MAX_CAPTURE_TAIL_ENERGY;
    let no_denominator_dilution = conservative_discarded <= MAX_DISCARDED_ENERGY
        && conservative_extra_suffix <= MAX_CAPTURE_TAIL_ENERGY;
    let pass = timing_pass
        && retained_peak
        && discarded <= MAX_DISCARDED_ENERGY
        && complete
        && no_denominator_dilution;
    let mut result = json!({"stage":"actual_aligned_isolated_response","label":label,"marker":marker,
        "actual_returned_pcm":true,"target":capture.target,"Dnew":capture.raw_cut,"N":capture.native_delay,
        "Cnom":capture.native_delay+BLOCK-1,"output_frames":OUTPUT,"full_raw_frames":capture.raw[0].len(),
        "original_raw_control_frames":original_raw_energy.len(),"original_raw_prefix_bitwise_equal":true,
        "raw_full_energy":raw_total,"original_raw_control_energy":original_raw_total,
        "added_raw_suffix_energy":added_suffix_energy,"added_raw_suffix_fraction":added_suffix_energy/raw_total,
        "complete_raw_sha256":interleaved_hash(&capture.raw),"original_raw_control_sha256":interleaved_hash(&aligned.original_raw),
        "actual_emitted_sha256":interleaved_hash(&capture.output),"independent_dry_sha256":interleaved_hash(&capture.dry)});
    let metrics = json!({"dry_quantiles":dry_q,"actual_emitted_quantiles":wet_q,"actual_errors_q10_q50_q90":errors,
        "timing_budget_frames":96,"timing_q10_q50_pass":timing_pass,"original_raw_peak_frame":raw_peak_frame,
        "original_raw_peak":raw_peak,"actual_emitted_peak_frame":peak_frame,"actual_emitted_peak":peak,
        "original_raw_peak_retained":retained_peak,"discarded_full_raw_energy_fraction":discarded,
        "discarded_original_control_energy_fraction":conservative_discarded,"discard_budget":MAX_DISCARDED_ENERGY,
        "actual_fixed_final_20ms_energy_fraction":tail,"extra_raw_suffix_frames":capture.raw[0].len()-capture.raw_cut-OUTPUT,
        "extra_raw_suffix_energy_fraction":extra_suffix,"extra_raw_suffix_original_control_fraction":conservative_extra_suffix,
        "tail_budget":MAX_CAPTURE_TAIL_ENERGY,"bounded_response_complete":complete,
        "no_denominator_dilution":no_denominator_dilution,"pass":pass,
        "fitted_offset_gain_crop":false,"internal_native_tail_exhaustion_proven":false,
        "ordinary_mixer_or_deadline_acceptance":false,"public_native_admission":false,
        "device_music_human_or_full_goal_acceptance":false});
    result
        .as_object_mut()
        .unwrap()
        .extend(metrics.as_object().unwrap().clone());
    aligned_emit(result);
    pass
}

#[test]
fn five_span_reference_keeps_original_prefix_and_distinguishes_the_old_wrap() {
    for frame in 0..OUTPUT {
        assert_eq!(five_span_coordinate(frame), coordinate(frame));
    }
    assert_eq!(coordinate(OUTPUT), 0.0);
    assert_eq!(five_span_coordinate(OUTPUT), SOURCE as f64);
    assert_eq!(
        five_span_coordinate(OUTPUT + EXPECTED_COMPENSATION),
        169223.5
    );
    assert_eq!(
        five_span_coordinate(EXTENDED_LOOP_OUTPUT),
        EXTENDED_SOURCE as f64
    );
    const { assert!(OUTPUT + EXPECTED_COMPENSATION < EXTENDED_LOOP_OUTPUT) };
    let seconds = five_span_seconds();
    assert_eq!(
        &seconds[..5],
        GeometryProfile::DeclaredAdjacentSupported.seconds()
    );
    assert_eq!(seconds.len(), 6);
}

#[test]
#[ignore = "explicit bounded actual original-owned Medium +7 aligned birth; no public or whole-domain acceptance"]
fn render_original_owned_medium_p7_aligned_birth() {
    aligned_emit(json!({"stage":"declared_eight_case_matrix_before_signal",
        "profile":NativeOutputProfile::Medium.label(),"shift":ALIGNED_SHIFT,"markers":[0,73728],
        "native_block_phases":[0,17],"callers":["512","irregular"],"captures":8,
        "source_frames":EXTENDED_SOURCE,"new_loop_output_frames":EXTENDED_LOOP_OUTPUT,
        "fixed_output_frames":OUTPUT,"N":EXPECTED_DELAY,"Cnom":EXPECTED_COMPENSATION,
        "identity":crate::audio_engine::pitch_runtime_tests::native_identity_tests::identify_loaded_native()
            .expect("actual linked Native DLL identity must be available")}));
    let mut cases = 0;
    let mut acoustic_passes = 0;
    for marker in [0, 73728] {
        let original = five_span_original(marker);
        for block_phase in [0, 17] {
            let mut previous: Option<AlignedCapture> = None;
            for caller in CALLERS {
                let aligned = capture_aligned(&original, block_phase, caller);
                if let Some(prior) = &previous {
                    assert_eq!(aligned.capture.output, prior.capture.output);
                    assert_eq!(aligned.capture.dry, prior.capture.dry);
                    assert_eq!(aligned.capture.raw, prior.capture.raw);
                    assert_eq!(aligned.original_raw, prior.original_raw);
                    assert_eq!(
                        interleaved_hash(&aligned.capture.output),
                        interleaved_hash(&prior.capture.output)
                    );
                    assert_eq!(
                        interleaved_hash(&aligned.capture.dry),
                        interleaved_hash(&prior.capture.dry)
                    );
                    assert_eq!(
                        interleaved_hash(&aligned.capture.raw),
                        interleaved_hash(&prior.capture.raw)
                    );
                    assert_eq!(
                        interleaved_hash(&aligned.original_raw),
                        interleaved_hash(&prior.original_raw)
                    );
                }
                let label = format!(
                    "five-span_IsolatedAttack({marker})_medium_k7_blockphase{block_phase}_{}",
                    caller.0
                );
                acoustic_passes += usize::from(aligned_metrics(&label, marker, &aligned));
                previous = Some(aligned);
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 8);
    aligned_emit(
        json!({"stage":"actual_aligned_component_complete","cases":cases,"acoustic_passes":acoustic_passes,
        "fixed_criteria_count":8,"public_native_admission":false,"ordinary_mixer_or_deadline_acceptance":false,
        "device_music_human_or_full_goal_acceptance":false}),
    );
    assert_eq!(
        acoustic_passes, 8,
        "actual aligned output must meet every unchanged bounded acoustic criterion"
    );
}
