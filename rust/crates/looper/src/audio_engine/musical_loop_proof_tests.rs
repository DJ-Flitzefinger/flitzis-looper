//! G3c: actual accepted mixer audio, independent physical and musical onset oracles.
//!
//! Passing physical-loop assertions are not musical acceptance. The ignored acceptance
//! probe deliberately retains the required one-loaded-frame bound against musical truth.

use super::super::mixer::RtMixer;
use super::tests::{accept_message, queue};
use super::*;
use flitzis_looper_analysis::tempo_evidence::{
    BeatThisModelIdentity, BeatThisRawEvidence, BeatThisRequestIdentity,
};
use serde_json::json;

const QUARTERS: usize = 64;
const SUBDIVISIONS: usize = 16;
const START_NUMERATOR: u64 = 41;
const START_DENOMINATOR: u64 = 4;
const PULSE_FRAMES: usize = 4;
const THRESHOLD: f32 = 0.25;

#[derive(Clone, Copy)]
struct Case {
    sample_rate: u32,
    quarter_numerator: u64,
    quarter_denominator: u64,
    rate_numerator: u64,
    rate_denominator: u64,
    label: &'static str,
}

impl Case {
    fn period_seconds(self) -> f64 {
        self.quarter_numerator as f64
            / self.quarter_denominator as f64
            / f64::from(self.sample_rate)
    }

    fn ratio(self) -> f64 {
        self.rate_numerator as f64 / self.rate_denominator as f64
    }

    fn musical_loop(self) -> (u64, u64) {
        (
            self.quarter_numerator,
            self.quarter_denominator * SUBDIVISIONS as u64,
        )
    }

    fn physical_end(self) -> usize {
        let (num, den) = self.musical_loop();
        nearest_positive_integer(
            START_NUMERATOR * den + num * START_DENOMINATOR,
            START_DENOMINATOR * den,
        )
    }

    fn physical_start(self) -> usize {
        nearest_positive_integer(START_NUMERATOR, START_DENOMINATOR)
    }

    fn physical_length(self) -> usize {
        self.physical_end() - self.physical_start()
    }

    /// Independent integer strict-threshold crossing, including the half-frame
    /// linear interpolation into the first pulse across the physical loop edge.
    fn onset(self, cycle: usize, musical: bool) -> usize {
        if cycle == 0 {
            return 0;
        }
        let (num, den) = if musical {
            self.musical_loop()
        } else {
            (self.physical_length() as u64, 1)
        };
        let top = (2 * cycle as u64 * num - den) * self.rate_denominator;
        let bottom = 2 * den * self.rate_numerator;
        (top / bottom + 1) as usize
    }

    fn rendered_frames(self, cycles: usize) -> usize {
        self.onset(cycles, false) + 16
    }
}

fn nearest_positive_integer(numerator: u64, denominator: u64) -> usize {
    let whole = numerator / denominator;
    let remainder = numerator % denominator;
    (whole
        + u64::from(
            2 * remainder > denominator || (2 * remainder == denominator && whole % 2 != 0),
        )) as usize
}

/// This fixture uses the existing production acceptance/ownership route. Backend
/// timestamps and musical-unit assertions are explicitly independently generated;
/// no inference, device or default-estimator acceptance is claimed.
fn accepted_mixer(case: Case) -> (AudioEngine, ConstantTimingTicket, RtMixer) {
    Python::initialize();
    let period = case.period_seconds();
    let start_seconds =
        START_NUMERATOR as f64 / START_DENOMINATOR as f64 / f64::from(case.sample_rate);
    let source_frames = (start_seconds * f64::from(case.sample_rate)
        + QUARTERS as f64 * case.quarter_numerator as f64 / case.quarter_denominator as f64)
        .ceil() as usize
        + PULSE_FRAMES
        + 2;
    let mut samples = vec![0.0; source_frames];
    let (loop_num, loop_den) = case.musical_loop();
    for pulse in 0..QUARTERS * SUBDIVISIONS {
        let frame = nearest_positive_integer(
            START_NUMERATOR * loop_den + pulse as u64 * loop_num * START_DENOMINATOR,
            START_DENOMINATOR * loop_den,
        );
        samples[frame..frame + PULSE_FRAMES].fill(0.5);
    }
    let sample = SampleBuffer {
        channels: 1,
        samples: Arc::from(samples),
    };
    let engine = AudioEngine::new().unwrap();
    engine.sample_cache.lock().unwrap()[0] = Some(sample.clone());
    engine.pad_request_ids.lock().unwrap()[0] = 7;
    engine.loaded_source_generations.lock().unwrap()[0] = (7, case.sample_rate);
    engine.loaded_source_digests.lock().unwrap()[0] = Some("a".repeat(64));
    engine.timing_intents.lock().unwrap()[0] = TimingIntent::Automatic;
    engine
        .input_runtime_ownership
        .set_timing_intent(0, TimingIntent::Automatic);
    engine
        .input_runtime_ownership
        .publish_source(0, &sample, case.sample_rate, 7);
    let binding = PcmBinding::verify(
        &sample.samples,
        PcmBindingMetadata {
            job: JobIdentity {
                pad_id: 0,
                request_id: 7,
                source_id: "g3c-generated-source-0-7".into(),
                source_generation: 7,
            },
            source_sha256: "a".repeat(64),
            source_provenance: "synthetic generated PCM; no original-file lineage claim".into(),
            pcm_sha256: f32_pcm_sha256(&sample.samples),
            sample_rate_hz: case.sample_rate,
            frame_count: source_frames as u64,
            origin_seconds: 0.0,
            mono_revision: MONO_REVISION.into(),
        },
    )
    .unwrap();
    let expected = BeatThisRequestIdentity {
        job: binding.metadata().job.clone(),
        pcm_sha256: binding.metadata().pcm_sha256.clone(),
        pcm_path: "g3c-generated-fixture.f32le".into(),
        sample_rate_hz: case.sample_rate,
        frame_count: source_frames as u64,
        origin_seconds: 0.0,
        dtype: "float32-le".into(),
        channels: 1,
        schema_version: 1,
        model: BeatThisModelIdentity {
            sha256: "b".repeat(64),
            frontend_id: "synthetic-g3c-no-inference".into(),
            environment_id: "independently-generated-fixture".into(),
            package_version: "1.1.0".into(),
            checkpoint: "final0".into(),
            postprocessor: "minimal".into(),
            device: "cpu".into(),
            precision: "float32".into(),
        },
    };
    let raw = BeatThisRawEvidence {
        response_job: expected.job.clone(),
        response_model: expected.model.clone(),
        response_schema_version: 1,
        expected_request: expected,
        beat_seconds: (0..QUARTERS)
            .map(|i| start_seconds + i as f64 * period)
            .collect(),
        downbeat_seconds: vec![start_seconds],
        beat_logits: vec![0.0],
        downbeat_logits: vec![-0.0],
    };
    let evidence = BoundTempoEvidence::from_beat_this(
        &binding,
        raw,
        TimingBound {
            halfwidth_seconds: 0.5 / f64::from(case.sample_rate),
            provenance: "independent generated quarter timestamps; half-frame declaration".into(),
        },
        start_seconds,
    )
    .unwrap();
    let mut guard = TimingAdoptionGuard::new(&binding, TimingIntent::Automatic).unwrap();
    let adoption_ticket = guard.issue_ticket().unwrap();
    let ticket = ConstantTimingTicket {
        id: 0,
        request_id: 7,
        source_generation: 7,
        sample_rate_hz: case.sample_rate,
        source_digest: "a".repeat(64),
        sample: sample.clone(),
        epoch: engine.prepared_source_epochs[0].clone(),
        captured_epoch: engine.prepared_source_epochs[0].load(Ordering::Acquire),
        binding: binding.metadata().clone(),
        evidence,
        guard: Mutex::new(guard),
        adoption_ticket,
        publication: Mutex::new(None),
    };
    let hypotheses = json!([{
        "id": "independent-generated-quarters",
        "provenance": "fixture defines each raw timestamp as a quarter; PCM pulses at 1/16quarter",
        "verification": "verified",
        "quarter_note_denominator": 1,
        "quarter_counts": (0..QUARTERS).map(|i| Some(i as i64)).collect::<Vec<_>>()
    }])
    .to_string();
    let (producer, mut consumer) = queue(2);
    let mut mixer = RtMixer::new(1, case.sample_rate as f32);
    mixer.set_current_timing_acknowledgements(engine.current_timing_acknowledgements.clone());
    mixer.set_input_runtime_ownership(engine.input_runtime_ownership.clone());
    mixer.set_prepared_source_epochs(engine.prepared_source_epochs.clone());
    mixer.load_sample(0, sample.clone());
    publish(
        &engine,
        &producer,
        &ticket,
        &hypotheses,
        IndependentTimingOrigin {
            seconds: start_seconds,
            provenance: "independently defined fractional synthetic source origin".into(),
        },
        TimingAcceptanceDecision {
            policy_version: "g3c-independent-fixture-acceptance-v1".into(),
            provenance: "synthetic quarter truth; no musical-loop or default acceptance".into(),
        },
    )
    .unwrap();
    assert_eq!(engine.current_timing_acknowledgements.current_epoch(0), 0);
    assert!(accept_message(&mut mixer, consumer.pop().unwrap()));
    assert_eq!(ticket.publication_status().unwrap(), "accepted");
    Python::attach(|py| {
        let current = current_metadata(&engine, py, 0).unwrap().unwrap();
        let dict = current.bind(py).cast::<PyDict>().unwrap();
        let get = |key| dict.get_item(key).unwrap().unwrap();
        assert_eq!(get("source_generation").extract::<u64>().unwrap(), 7);
        assert_eq!(
            get("sample_rate_hz").extract::<u32>().unwrap(),
            case.sample_rate
        );
        assert_eq!(
            get("frame_count").extract::<usize>().unwrap(),
            source_frames
        );
        let actual_period = get("period_seconds_per_quarter").extract::<f64>().unwrap();
        assert!((actual_period - period).abs() * f64::from(case.sample_rate) < 1e-8);
    });
    // The productive rate resolver must consume this acknowledged accepted period,
    // rather than obtaining an equal value from an unrelated legacy BPM or speed.
    let accepted_period = ticket
        .guard
        .lock()
        .unwrap()
        .accepted()
        .unwrap()
        .period_seconds_per_quarter();
    mixer.set_bpm_lock(true);
    mixer.set_master_period(accepted_period / case.ratio());
    mixer.set_pad_loop_region(
        0,
        case.physical_start() as f64 / f64::from(case.sample_rate),
        Some(case.physical_end() as f64 / f64::from(case.sample_rate)),
    );
    assert_eq!(
        mixer.loop_region_frames(0),
        (case.physical_start(), Some(case.physical_end()))
    );
    assert!(mixer.play_sample(0, 1.0));
    assert!(Arc::ptr_eq(
        &mixer.voices[0].sample.as_ref().unwrap().samples,
        &sample.samples
    ));
    assert_eq!(
        mixer.voices[0]
            .source_timing
            .accepted
            .unwrap()
            .publication_epoch,
        engine.current_timing_acknowledgements.current_epoch(0)
    );
    assert!((mixer.voices[0].source_playback.tempo_ratio() - case.ratio()).abs() < 1e-14);
    (engine, ticket, mixer)
}

fn render(case: Case, partition: &[usize], cycles: usize) -> (Vec<f32>, serde_json::Value) {
    let (engine, ticket, mut mixer) = accepted_mixer(case);
    let frames = case.rendered_frames(cycles);
    let mut output = vec![0.0; frames];
    let mut offset = 0;
    let mut callback = 0;
    let mut peaks = [0.0; super::super::constants::NUM_SAMPLES];
    while offset < frames {
        let count = partition[callback % partition.len()].min(frames - offset);
        mixer.render_at_output_frame(
            offset as u64,
            &mut output[offset..offset + count],
            &mut peaks,
        );
        offset += count;
        callback += 1;
    }
    let position = mixer.voices[0].source_playback.position();
    let distance_num = frames as u64 * case.rate_numerator;
    let expected_frame = case.physical_start()
        + ((distance_num / case.rate_denominator) % case.physical_length() as u64) as usize;
    let expected_fraction =
        (distance_num % case.rate_denominator) as f64 / case.rate_denominator as f64;
    assert_eq!(position.frame, expected_frame);
    assert!((position.fraction - expected_fraction).abs() < 1e-8);
    assert_eq!(mixer.voices[0].frame_pos, expected_frame);
    assert_eq!(
        position.seek_mode,
        super::super::source_reader::ExplicitSeekMode::Normal
    );
    let actual_source_beat = mixer.active_pad_beat_position(0).unwrap();
    let expected_source_beat = (expected_frame as f64 + expected_fraction
        - START_NUMERATOR as f64 / START_DENOMINATOR as f64)
        / (case.quarter_numerator as f64 / case.quarter_denominator as f64);
    assert!((actual_source_beat - expected_source_beat).abs() < 1e-12);
    let onsets: Vec<usize> = output
        .iter()
        .enumerate()
        .filter_map(|(i, &sample)| {
            (sample > THRESHOLD && (i == 0 || output[i - 1] <= THRESHOLD)).then_some(i)
        })
        .collect();
    let expected_onsets: Vec<usize> = (0..=cycles).map(|k| case.onset(k, false)).collect();
    assert_eq!(
        onsets,
        expected_onsets,
        "{} Fs{} r{}",
        case.label,
        case.sample_rate,
        case.ratio()
    );
    let mut max_sample_error = 0.0_f32;
    for (frame, &actual) in output.iter().enumerate() {
        let num = frame as u64 * case.rate_numerator;
        let source = (num / case.rate_denominator) as usize % case.physical_length();
        let next = (source + 1) % case.physical_length();
        let left = if source < PULSE_FRAMES { 0.5_f32 } else { 0.0 };
        let right = if next < PULSE_FRAMES { 0.5_f32 } else { 0.0 };
        let fraction = (num % case.rate_denominator) as f32 / case.rate_denominator as f32;
        let expected = left + (right - left) * fraction;
        max_sample_error = max_sample_error.max((actual - expected).abs());
    }
    assert!(max_sample_error <= 1e-7, "sample error {max_sample_error}");
    let accepted = ticket.guard.lock().unwrap().accepted().unwrap().clone();
    let (musical_num, musical_den) = case.musical_loop();
    let checkpoints: Vec<_> = [75, 1000]
        .into_iter()
        .filter(|&cycle| cycle <= cycles)
        .map(|cycle| {
            let physical = case.onset(cycle, false);
            let musical = case.onset(cycle, true);
            let delta = cycle as f64
                * (case.physical_length() as f64 - musical_num as f64 / musical_den as f64)
                / case.ratio();
            let samples: Vec<_> = (physical.saturating_sub(2)..physical + 7)
                .map(|frame| json!({"output_frame": frame, "sample": output[frame]}))
                .collect();
            json!({
                "cycle": cycle,
                "actual_onset": onsets[cycle],
                "physical_oracle_onset": physical,
                "musical_oracle_onset": musical,
                "onset_error_output_frames": onsets[cycle] as i64 - musical as i64,
                "cumulative_boundary_error_output_frames": delta,
                "cumulative_boundary_error_loaded_frames": delta * case.ratio(),
                "musical_gate": (delta * case.ratio()).abs() <= 1.0,
                "actual_samples": samples
            })
        })
        .collect();
    let mut row = json!({
        "case": case.label, "sample_rate_hz": case.sample_rate,
        "quarter_loaded_frames_numerator": case.quarter_numerator,
        "quarter_loaded_frames_denominator": case.quarter_denominator,
        "musical_loop_loaded_frames_numerator": musical_num,
        "musical_loop_loaded_frames_denominator": musical_den,
        "period_seconds": accepted.period_seconds_per_quarter(),
        "period_seconds_bits": format!("{:016x}", accepted.period_seconds_per_quarter().to_bits()),
        "origin_loaded_frames_numerator": START_NUMERATOR,
        "origin_loaded_frames_denominator": START_DENOMINATOR,
        "origin_seconds_bits": format!("{:016x}", accepted.origin().seconds.to_bits()),
        "rate_numerator": case.rate_numerator, "rate_denominator": case.rate_denominator,
        "rate": case.ratio(), "rate_bits": format!("{:016x}", case.ratio().to_bits()),
        "admitted_rate_bits": format!("{:016x}", mixer.voices[0].source_playback.tempo_ratio().to_bits()),
        "bpm_lock": true,
        "physical_start_frame": case.physical_start(), "physical_end_frame": case.physical_end(),
        "physical_loop_frames": case.physical_length(), "callback_partition": partition,
        "rendered_output_frames": frames, "callback_count": callback,
        "source_generation": 7, "source_id": ticket.binding.job.source_id,
        "source_sha256": ticket.binding.source_sha256, "pcm_sha256": ticket.binding.pcm_sha256,
        "source_frame_count": ticket.binding.frame_count, "accepted_revision": accepted.revision(),
        "acknowledged_epoch": engine.current_timing_acknowledgements.current_epoch(0),
        "current_metadata_available": Python::attach(|py| current_metadata(&engine, py, 0).unwrap().is_some()),
        "source_pin_matches": Arc::ptr_eq(&mixer.voices[0].sample.as_ref().unwrap().samples, &ticket.sample.samples),
        "final_source_frame": position.frame, "final_source_fraction": position.fraction,
        "max_sample_error": max_sample_error, "threshold": THRESHOLD, "pulse_amplitude": 0.5,
        "pulse_source_frames": PULSE_FRAMES, "actual_onsets": onsets,
        "checkpoints": checkpoints,
        "musical_gate": ([75, 1000].into_iter().filter(|&k| k <= cycles)
            .all(|k| (k as f64 * (case.physical_length() as f64 - musical_num as f64 / musical_den as f64)).abs() <= 1.0))
    });
    row["final_wrapped_source_beat"] = json!(actual_source_beat);
    row["independent_final_wrapped_source_beat"] = json!(expected_source_beat);
    row["unwrapped_source_distance_frames"] =
        json!(distance_num as f64 / case.rate_denominator as f64);
    (output, row)
}

fn export(name: &str, rows: &[serde_json::Value]) {
    if let Some(directory) = std::env::var_os("FLITZIS_G3C_OUTPUT_DIR") {
        let path = std::path::PathBuf::from(directory);
        let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(4)
            .unwrap()
            .canonicalize()
            .unwrap();
        assert!(path.is_absolute());
        let within_evidence = |path: &std::path::Path| {
            path.starts_with(workspace.join("scratch"))
                || path.starts_with(workspace.join("exports"))
        };
        assert!(within_evidence(
            &path.parent().unwrap().canonicalize().unwrap()
        ));
        std::fs::create_dir_all(&path).unwrap();
        let path = path.canonicalize().unwrap();
        assert!(within_evidence(&path));
        let evidence = json!({
            "schema": "g3c-generated-accepted-mixer-proof-v1",
            "claim": "physical rendered/partition evidence; musical acceptance is separately gated",
            "device_or_listening_evidence": false,
            "rows": rows
        });
        std::fs::write(
            path.join(name),
            serde_json::to_vec_pretty(&evidence).unwrap(),
        )
        .unwrap();
    }
}

#[test]
fn actual_accepted_rendered_fractional_loops_match_physical_oracle_across_75_and_1000_cycles() {
    let mut rows = Vec::new();
    for sample_rate in [44_100, 48_000, 96_000] {
        for (rate_numerator, rate_denominator) in [(73, 100), (3, 4), (5, 4)] {
            let case = Case {
                sample_rate,
                quarter_numerator: u64::from(sample_rate) + 8,
                quarter_denominator: 2,
                rate_numerator,
                rate_denominator,
                label: "rational-quarter-0.5-plus-4-frames",
            };
            let (reference, row) = render(case, &[512], 1000);
            rows.push(row);
            for partition in [&[1, 17, 257, 64, 1023, 3][..], &[1][..]] {
                let (actual, row) = render(case, partition, 1000);
                assert_eq!(
                    actual, reference,
                    "callback partition changes rendered audio"
                );
                rows.push(row);
            }
        }
    }
    export("generated-rational-rendered-proof.json", &rows);
}

#[test]
fn actual_accepted_119999_and_12345_bpm_rendered_onsets_preserve_fractional_physical_truth() {
    let mut rows = Vec::new();
    for sample_rate in [44_100, 48_000, 96_000] {
        for (bpm_denominator, bpm_scale, label) in
            [(119_999, 1000, "bpm-119.999"), (12_345, 100, "bpm-123.45")]
        {
            for (rate_numerator, rate_denominator) in [(73, 100), (5, 4)] {
                let case = Case {
                    sample_rate,
                    quarter_numerator: u64::from(sample_rate) * 60 * bpm_scale,
                    quarter_denominator: bpm_denominator,
                    rate_numerator,
                    rate_denominator,
                    label,
                };
                let (reference, row) = render(case, &[512], 1000);
                rows.push(row);
                for partition in [&[1, 17, 257, 64, 1023, 3][..], &[1][..]] {
                    let (actual, row) = render(case, partition, 1000);
                    assert_eq!(actual, reference);
                    rows.push(row);
                }
            }
        }
    }
    export("generated-realistic-rendered-proof.json", &rows);
}

#[test]
#[ignore = "G3c musical acceptance requires <=1 loaded-frame cumulative error; current physical wrap fails"]
fn strict_musical_rendered_onset_acceptance_gate_remains_one_source_frame_after_75_and_1000_cycles()
{
    let case = Case {
        sample_rate: 48_000,
        quarter_numerator: 48_008,
        quarter_denominator: 2,
        rate_numerator: 73,
        rate_denominator: 100,
        label: "strict-musical-acceptance",
    };
    let (_, row) = render(case, &[1, 17, 257, 64, 1023, 3], 1000);
    export("strict-musical-acceptance-gate.json", &[row.clone()]);
    let failures: Vec<_> = row["checkpoints"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|checkpoint| {
            checkpoint["cumulative_boundary_error_loaded_frames"]
                .as_f64()
                .unwrap()
                .abs()
                > 1.0
        })
        .collect();
    assert!(
        failures.is_empty(),
        "required musical gate failed: {failures:?}"
    );
}
