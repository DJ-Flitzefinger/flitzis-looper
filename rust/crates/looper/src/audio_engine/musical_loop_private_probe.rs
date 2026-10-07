//! Explicit hardware-free G3c gate for the unchanged private 600-second WAV.
//!
//! Registered below `constant_timing` so the retained real backend evidence can
//! enter the same guarded publication as productive preparation. This test-only
//! bridge does not bypass adoption with a fabricated accepted projection. The
//! separate budget probe executes actual full-source native QM preparation.
//! Neither probe establishes default-analyzer, device or listening acceptance.

use super::*;
use crate::audio_engine::buffer_retirement::ImmediateAudioBufferRetirement;
use crate::audio_engine::mixer::RtMixer;
use crate::audio_engine::prepared_source::file_sha256;
use crate::audio_engine::sample_loader::decode_audio_file_to_sample_buffer;
use crate::audio_engine::{LoadedSourcePublication, publish_loaded_sample};
use crate::messages::StemMixMode;
use base64::Engine;
use flitzis_looper_analysis::tempo_evidence::{
    BeatThisModelIdentity, BeatThisRawEvidence, BeatThisRequestIdentity,
};
use flitzis_looper_analysis::tempo_refinement::IndependentQuarterEvidence;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

const RATE: u32 = 48_000;
const SOURCE_FRAMES: usize = 28_800_000;
const LOOP_FRAMES: usize = 24_000;
const ORIGINAL_SHA256: &str = "96ffe98cf44215719b0b57d605d6dc586c9c4e763ad3d47d512d0ba787d204ef";
const THRESHOLD: f32 = 0.05;
const SAMPLE_ERROR_LIMIT: f64 = 2.0e-6;
const PARTITIONS: [&[usize]; 2] = [&[512], &[1, 127, 384, 96, 257, 512, 31]];

fn text(value: &Value, key: &str) -> String {
    value[key].as_str().expect("declared string field").into()
}

fn integer(value: &Value, key: &str) -> u64 {
    value[key].as_u64().expect("declared integer field")
}

fn original_job(value: &Value) -> JobIdentity {
    JobIdentity {
        pad_id: integer(value, "pad_id"),
        request_id: integer(value, "request_id"),
        source_id: text(value, "source_id"),
        source_generation: integer(value, "source_generation"),
    }
}

fn model(value: &Value) -> BeatThisModelIdentity {
    BeatThisModelIdentity {
        sha256: text(value, "sha256"),
        frontend_id: text(value, "frontend_id"),
        environment_id: text(value, "environment_id"),
        package_version: text(value, "package_version"),
        checkpoint: text(value, "checkpoint"),
        postprocessor: text(value, "postprocessor"),
        device: text(value, "device"),
        precision: text(value, "precision"),
    }
}

fn lossless_array(gate: &Value, key: &str) -> Vec<f64> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(text(&gate["beat_this_packed_predictions"], key))
        .unwrap();
    assert_eq!(bytes.len() % 8, 0);
    let values: Vec<_> = bytes
        .chunks_exact(8)
        .map(|item| f64::from_le_bytes(item.try_into().unwrap()))
        .collect();
    // Frozen G2 lossless array identities anchor these actual retained values;
    // changing an input array and its adjacent declared hash cannot pass.
    let expected = match key {
        "beat_seconds" | "downbeat_seconds" => {
            "46e63909d4ab134a031f7f783358e0d2d2716af1f7a64d98ae558fb6a9266cdd"
        }
        "beat_logits" => "615e1d59f6355d5f7ca6390daa1ee908754a8e5dd0319df885af711951afd048",
        "downbeat_logits" => "103dee29d1d0ea36710c4d2f31f1d4fb17f0d9a9489221e35bd4f86132757d26",
        _ => panic!("unexpected retained complete array"),
    };
    assert_eq!(f64_input_sha256(&values), expected);
    assert_eq!(text(&gate["beat_this_array_sha256"], key), expected);
    values
}

fn independent_quarters(gate: &Value) -> IndependentQuarterEvidence {
    let declared = &gate["independent_quarters"];
    IndependentQuarterEvidence {
        source_sha256: text(gate, "source_sha256"),
        pcm_sha256: text(gate, "pcm_sha256"),
        loaded_sample_rate_hz: RATE,
        loaded_frame_count: SOURCE_FRAMES as u64,
        feature_policy_version: flitzis_looper_analysis::tempo_refinement::POLICY_VERSION.into(),
        feature_frames: declared["feature_frames"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap())
            .collect(),
        quarter_count_numerators: declared["quarter_count_numerators"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_i64().unwrap())
            .collect(),
        quarter_note_denominator: 1,
        provenance: text(declared, "provenance"),
    }
}

fn retained_evidence(binding: &PcmBinding<'_>, gate: &Value) -> BoundTempoEvidence {
    let request = &gate["beat_this_request"];
    let expected_request = BeatThisRequestIdentity {
        job: original_job(&request["identity"]),
        pcm_sha256: text(gate, "pcm_sha256"),
        pcm_path: text(&request["pcm"], "path"),
        sample_rate_hz: RATE,
        frame_count: SOURCE_FRAMES as u64,
        origin_seconds: 0.0,
        dtype: "float32-le".into(),
        channels: 1,
        schema_version: 1,
        model: model(&request["model"]),
    };
    BoundTempoEvidence::from_beat_this(
        binding,
        BeatThisRawEvidence {
            response_job: expected_request.job.clone(),
            response_model: expected_request.model.clone(),
            response_schema_version: 1,
            expected_request,
            beat_seconds: lossless_array(gate, "beat_seconds"),
            downbeat_seconds: lossless_array(gate, "downbeat_seconds"),
            beat_logits: lossless_array(gate, "beat_logits"),
            downbeat_logits: lossless_array(gate, "downbeat_logits"),
        },
        TimingBound {
            halfwidth_seconds: 0.05,
            provenance:
                "retained G2 explicit 50ms engineering feature search; no musical confidence claim"
                    .into(),
        },
        0.0,
    )
    .unwrap()
}

fn publish_actual_acceptance(
    engine: &AudioEngine,
    mixer: &mut RtMixer,
    sample: &SampleBuffer,
    gate: &Value,
) -> (String, u64) {
    let original_job = original_job(&gate["beat_this_request"]["identity"]);
    assert_eq!(original_job.pad_id, 0);
    assert_ne!(original_job.request_id, 0);
    assert_ne!(original_job.source_generation, 0);
    let (producer, mut consumer) = rtrb::RingBuffer::new(2);
    let producer = Arc::new(Mutex::new(producer));
    engine.pad_request_ids.lock().unwrap()[0] = original_job.request_id;
    engine.timing_intents.lock().unwrap()[0] = TimingIntent::Automatic;
    engine
        .input_runtime_ownership
        .set_timing_intent(0, TimingIntent::Automatic);
    let mut generations = engine.loaded_source_generations.lock().unwrap();
    let mut digests = engine.loaded_source_digests.lock().unwrap();
    publish_loaded_sample(
        &producer,
        &engine.sample_cache,
        0,
        sample.clone(),
        LoadedSourcePublication {
            cold: false,
            cold_epoch: None,
            cold_adoption: None,
            replace_assignment: false,
            loop_region: None,
            resident_cancelled: None,
            intent: None,
            ownership: &engine.input_runtime_ownership,
            generation: original_job.source_generation,
            rate: RATE,
            generation_slot: &mut generations[0],
            digest_slot: &mut digests[0],
            digest: text(gate, "source_sha256"),
        },
    )
    .unwrap();
    drop(generations);
    drop(digests);
    let ControlMessage::LoadSample { id, sample: loaded } = consumer.pop().unwrap() else {
        panic!("native source publication must precede timing");
    };
    mixer.load_sample(id, loaded);
    assert!(
        engine
            .input_runtime_ownership
            .source_current(0, sample, RATE)
    );

    // Original retained job/backend identity stays intact. The fresh guard and
    // native current-source/request checks own this explicit fixture publication.
    let binding = PcmBinding::verify(
        &sample.samples,
        PcmBindingMetadata {
            job: original_job.clone(),
            source_sha256: text(gate, "source_sha256"),
            source_provenance: text(gate, "source_provenance"),
            pcm_sha256: text(gate, "pcm_sha256"),
            sample_rate_hz: RATE,
            frame_count: SOURCE_FRAMES as u64,
            origin_seconds: 0.0,
            mono_revision: MONO_REVISION.into(),
        },
    )
    .unwrap();
    let evidence = retained_evidence(&binding, gate);
    let timing = AcceptedConstantTiming::from_comparable_attacks(
        evidence.clone(),
        &binding,
        0.05,
        &independent_quarters(gate),
        IndependentTimingOrigin {
            seconds: 0.0,
            provenance: "independent fixture attack zero at actual original/loaded frame zero"
                .into(),
        },
        TimingAcceptanceDecision {
            policy_version: "private-g3c-complete-comparable-quarter-fixture-v1".into(),
            provenance: "retained independent G2 fixture-quarter assertion; explicit numeric renderer gate only"
                .into(),
        },
    )
    .unwrap();
    assert_eq!(timing.period_seconds_per_quarter(), 0.5);
    assert_eq!(timing.refinement().unwrap().attacks.len(), 1200);
    let revision = timing.revision().to_owned();
    let mut guard = TimingAdoptionGuard::new(&binding, TimingIntent::Automatic).unwrap();
    let adoption_ticket = guard.issue_ticket().unwrap();
    let ticket = ConstantTimingTicket {
        id: 0,
        request_id: original_job.request_id,
        source_generation: original_job.source_generation,
        sample_rate_hz: RATE,
        source_digest: text(gate, "source_sha256"),
        sample: sample.clone(),
        epoch: engine.prepared_source_epochs[0].clone(),
        captured_epoch: engine.prepared_source_epochs[0].load(Ordering::Acquire),
        binding: binding.metadata().clone(),
        evidence,
        guard: Mutex::new(guard),
        adoption_ticket,
        publication: Mutex::new(None),
        pcm_budget: PcmBudget::default(),
    };
    publish_record(engine, &producer, &ticket, timing, false).unwrap();
    Python::attach(|py| assert!(current_metadata(engine, py, 0).unwrap().is_none()));
    let ControlMessage::PublishConstantTiming { id, timing } = consumer.pop().unwrap() else {
        panic!("native timing publication");
    };
    assert!(mixer.publish_constant_timing_rt(id, timing, &mut ImmediateAudioBufferRetirement));
    assert_eq!(ticket.publication_status().unwrap(), "accepted");
    let epoch = engine.current_timing_acknowledgements.current_epoch(0);
    assert_ne!(epoch, 0);
    drop(ticket);
    Python::attach(|py| {
        let metadata = current_metadata(engine, py, 0).unwrap().unwrap();
        let metadata = metadata.bind(py).cast::<PyDict>().unwrap();
        assert_eq!(
            metadata
                .get_item("revision")
                .unwrap()
                .unwrap()
                .extract::<String>()
                .unwrap(),
            revision
        );
        assert_eq!(
            metadata
                .get_item("period_seconds_per_quarter")
                .unwrap()
                .unwrap()
                .extract::<f64>()
                .unwrap(),
            0.5
        );
    });
    (revision, epoch)
}

// Closed-form waveform oracle: no SourcePlayback, SourceGrid, source_reader,
// stretch buffers or mixer source-position helpers participate.
fn expected_sample(source: &[f32], output_frame: u64, ratio: f64) -> f32 {
    let distance = output_frame as f64 * ratio;
    let whole = distance.floor() as u64;
    let fraction = (distance - whole as f64) as f32;
    let lower = source[whole as usize % LOOP_FRAMES];
    let upper = source[(whole as usize + 1) % LOOP_FRAMES];
    lower + (upper - lower) * fraction
}

#[derive(Default)]
struct ThresholdOnsets {
    frames: Vec<u64>,
}

impl ThresholdOnsets {
    fn observe(&mut self, frame: u64, sample: f32, refractory_frames: u64) {
        if sample.abs() >= THRESHOLD
            && self
                .frames
                .last()
                .is_none_or(|last| frame - last >= refractory_frames)
        {
            self.frames.push(frame);
        }
    }
}

fn wave_bytes(samples: &[f32]) -> Vec<u8> {
    let pcm: Vec<u8> = samples.iter().flat_map(|v| v.to_le_bytes()).collect();
    let mut result = Vec::with_capacity(44 + pcm.len());
    result.extend_from_slice(b"RIFF");
    result.extend_from_slice(&(36 + pcm.len() as u32).to_le_bytes());
    result.extend_from_slice(b"WAVEfmt ");
    result.extend_from_slice(&16_u32.to_le_bytes());
    result.extend_from_slice(&3_u16.to_le_bytes()); // IEEE float, mono, 48kHz
    result.extend_from_slice(&1_u16.to_le_bytes());
    result.extend_from_slice(&RATE.to_le_bytes());
    result.extend_from_slice(&(RATE * 4).to_le_bytes());
    result.extend_from_slice(&4_u16.to_le_bytes());
    result.extend_from_slice(&32_u16.to_le_bytes());
    result.extend_from_slice(b"data");
    result.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    result.extend_from_slice(&pcm);
    result
}

fn duration_report(onsets: &[u64], cycles: usize, ratio: f64) -> Value {
    if onsets.len() <= cycles {
        return json!({"cycles": cycles, "error": "required boundary onset missing", "actual_onset_count": onsets.len()});
    }
    let errors: Vec<_> = onsets[..=cycles]
        .iter()
        .enumerate()
        .map(|(cycle, frame)| *frame as f64 - cycle as f64 * LOOP_FRAMES as f64 / ratio)
        .collect();
    let max_onset_error = errors.iter().copied().map(f64::abs).fold(0.0, f64::max);
    json!({
        "cycles": cycles,
        "musical_source_duration_frames": cycles as f64 * 0.5 * f64::from(RATE),
        "physical_source_duration_frames": cycles * LOOP_FRAMES,
        "physical_minus_musical_source_frames": 0.0,
        "expected_output_duration_frames": cycles as f64 * LOOP_FRAMES as f64 / ratio,
        "threshold_onset_output_frames": &onsets[..=cycles],
        "threshold_onset_minus_physical_crossing_output_frames": errors,
        "max_abs_threshold_onset_error_output_frames": max_onset_error,
        "threshold_onset_error_bound_output_frames": 1.0 / ratio + 1.0,
        "threshold_onset_error_bound_passed": max_onset_error <= 1.0 / ratio + 1.0,
        "cumulative_threshold_onset_minus_musical_output_frames": errors[cycles] - errors[0],
    })
}

fn render_case(
    mixer: &mut RtMixer,
    source: &[f32],
    ratio: f64,
    partition_index: usize,
    output_dir: &Path,
) -> Value {
    mixer.stop_sample(0);
    mixer.set_speed(ratio);
    mixer.set_bpm_lock(false);
    mixer.set_pad_key_lock(0, false);
    mixer.set_pad_gain(0, 0.0);
    mixer.set_pad_eq(0, 0.0, 0.0, 0.0);
    mixer.set_volume(1.0);
    assert!(mixer.set_stem_mix_mode(0, StemMixMode::FullMix, 0));
    mixer.set_pad_loop_region(0, 0.0, Some(0.5));
    assert_eq!(mixer.loop_region_frames(0), (0, Some(LOOP_FRAMES)));
    assert!(mixer.play_sample_at_output_frame(0, 1.0, 0));
    let end = (1000.0 * LOOP_FRAMES as f64 / ratio).ceil() as u64 + 1;
    let checkpoint = (75.0 * LOOP_FRAMES as f64 / ratio).ceil() as u64 + 1;
    let last_boundary = (1000.0 * LOOP_FRAMES as f64 / ratio).floor() as u64;
    let ranges = [
        (0, u64::from(RATE) * 2),
        (
            checkpoint.saturating_sub(u64::from(RATE) / 10),
            checkpoint + u64::from(RATE) / 10,
        ),
        (last_boundary.saturating_sub(u64::from(RATE) / 10), end),
    ];
    let mut snippets: [Vec<f32>; 3] = std::array::from_fn(|_| Vec::new());
    let refractory_frames = (LOOP_FRAMES as f64 / ratio / 2.0).floor() as u64;
    let mut actual_onsets = ThresholdOnsets::default();
    let mut oracle_onsets = ThresholdOnsets::default();
    let mut digest = Sha256::new();
    let mut maximum_error = 0.0_f64;
    let mut cursor_error = 0.0_f64;
    let mut elapsed = 0_u64;
    let mut partition = 0;
    let mut peaks = [0.0; NUM_SAMPLES];
    let mut output = [0.0_f32; 512];
    while elapsed < end {
        let mut frames = PARTITIONS[partition_index][partition % PARTITIONS[partition_index].len()]
            .min((end - elapsed) as usize);
        if elapsed < checkpoint {
            frames = frames.min((checkpoint - elapsed) as usize);
        }
        output[..frames].fill(0.0);
        mixer.render_at_output_frame(elapsed, &mut output[..frames], &mut peaks);
        for (index, actual) in output[..frames].iter().copied().enumerate() {
            let frame = elapsed + index as u64;
            let expected = expected_sample(source, frame, ratio);
            maximum_error = maximum_error.max((f64::from(actual) - f64::from(expected)).abs());
            actual_onsets.observe(frame, actual, refractory_frames);
            oracle_onsets.observe(frame, expected, refractory_frames);
            digest.update(actual.to_le_bytes());
            for (range_index, (start, stop)) in ranges.iter().enumerate() {
                if frame >= *start && frame < *stop {
                    snippets[range_index].push(actual);
                }
            }
        }
        elapsed += frames as u64;
        partition += 1;
        if elapsed == checkpoint || elapsed == end {
            let voice = mixer
                .voices
                .iter()
                .find(|v| v.is_playing_sample(0))
                .unwrap();
            assert!(voice.source_timing.accepted.is_some());
            let position = voice.source_playback.position();
            let distance = elapsed as f64 * ratio;
            let whole = distance.floor() as u64;
            assert_eq!(position.frame, whole as usize % LOOP_FRAMES);
            cursor_error = cursor_error.max((position.fraction - (distance - whole as f64)).abs());
            assert!(cursor_error <= 1.0e-8);
        }
    }
    let name = format!("rate-{ratio}-partition-{partition_index}");
    let mut snippet_records = Vec::new();
    for (index, samples) in snippets.iter().enumerate() {
        let wav = output_dir.join(format!("{name}-window-{index}.wav"));
        let f32le = output_dir.join(format!("{name}-window-{index}.f32le"));
        fs::write(&wav, wave_bytes(samples)).unwrap();
        fs::write(
            &f32le,
            samples
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        snippet_records.push(json!({
            "output_start_frame": ranges[index].0,
            "frame_count": samples.len(), "wav": wav, "float32_le": f32le,
            "pcm_sha256": f32_pcm_sha256(samples),
        }));
    }
    let report = json!({
        "rate": ratio, "rate_binary64_bits": ratio.to_bits(),
        "partitions": PARTITIONS[partition_index], "rendered_output_frames": end,
        "actual_rendered_float32_le_sha256": format!("{:x}", digest.finalize()),
        "maximum_rendered_sample_error": maximum_error,
        "maximum_rendered_sample_error_limit": SAMPLE_ERROR_LIMIT,
        "maximum_cursor_fraction_error_source_frames": cursor_error,
        "actual_threshold_onsets": actual_onsets.frames,
        "independent_waveform_oracle_threshold_onsets": oracle_onsets.frames,
        "onsets_equal": actual_onsets.frames == oracle_onsets.frames,
        "75_cycles": duration_report(&actual_onsets.frames, 75, ratio),
        "1000_cycles": duration_report(&actual_onsets.frames, 1000, ratio),
        "snippets": snippet_records,
    });
    // Save a reviewable failure before assertions, including actual/onset exports.
    fs::write(
        output_dir.join(format!("{name}.json")),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    assert!(
        maximum_error <= SAMPLE_ERROR_LIMIT,
        "{name}: {maximum_error}"
    );
    if ratio == 1.0 {
        assert_eq!(
            maximum_error, 0.0,
            "integer-rate positive control is bit exact"
        );
    }
    assert_eq!(actual_onsets.frames, oracle_onsets.frames, "{name}");
    assert_eq!(actual_onsets.frames.len(), 1001, "{name}");
    assert_eq!(
        report["75_cycles"]["threshold_onset_error_bound_passed"],
        true
    );
    assert_eq!(
        report["1000_cycles"]["threshold_onset_error_bound_passed"],
        true
    );
    report
}

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap()
        .canonicalize()
        .unwrap()
}

#[test]
#[ignore = "explicit unchanged private WAV gate; needs FLITZIS_G3C_WAV and FLITZIS_G3C_OUTPUT_DIR"]
fn private_actual_wav_accepted_physical_loop_render_and_onsets_75_1000_cycles() {
    let workspace = workspace();
    let wav = PathBuf::from(std::env::var_os("FLITZIS_G3C_WAV").unwrap())
        .canonicalize()
        .unwrap();
    assert!(wav.starts_with(workspace.join("test-audio")));
    let output_dir = PathBuf::from(std::env::var_os("FLITZIS_G3C_OUTPUT_DIR").unwrap());
    assert!(output_dir.is_absolute());
    let output_parent = output_dir.parent().unwrap().canonicalize().unwrap();
    assert!(
        output_parent.starts_with(workspace.join("scratch"))
            || output_parent.starts_with(workspace.join("exports"))
    );
    fs::create_dir_all(&output_dir).unwrap();
    let output_dir = output_dir.canonicalize().unwrap();
    assert!(
        output_dir.starts_with(workspace.join("scratch"))
            || output_dir.starts_with(workspace.join("exports"))
    );
    let gate_path = std::env::var_os("FLITZIS_G3C_GATE_INPUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace.join("scratch/g2b2-tempo-20261006/gate-input.json"));
    let gate_path = gate_path.canonicalize().unwrap();
    assert!(gate_path.starts_with(workspace.join("scratch")));
    assert!(fs::metadata(&gate_path).unwrap().len() <= 8 * 1024 * 1024);
    let gate: Value = serde_json::from_slice(&fs::read(&gate_path).unwrap()).unwrap();
    let retained_response = workspace
        .join("scratch/grid-timing-audit-20261006/beat-this-new-wav/worker-response.raw.json");
    let retained_response_sha256 = file_sha256(&retained_response).unwrap();
    assert_eq!(
        retained_response_sha256,
        "eccd4faadef1cb9bd30ed0523f9ee9973f99f759f6338c626e3a8db5d4ea0ba5"
    );
    assert_eq!(
        retained_response_sha256,
        text(&gate, "retained_response_sha256")
    );
    let response: Value = serde_json::from_slice(&fs::read(&retained_response).unwrap()).unwrap();
    assert_eq!(response["identity"], gate["beat_this_request"]["identity"]);
    assert_eq!(response["model"], gate["beat_this_request"]["model"]);
    assert_eq!(integer(&gate, "sample_rate_hz"), u64::from(RATE));
    assert_eq!(integer(&gate, "frame_count"), SOURCE_FRAMES as u64);
    let source_sha256 = file_sha256(&wav).unwrap();
    assert_eq!(source_sha256, ORIGINAL_SHA256);
    assert_eq!(source_sha256, text(&gate, "source_sha256"));
    let source = decode_audio_file_to_sample_buffer(&wav, 1, RATE, |_| {}).unwrap();
    assert_eq!(source.channels, 1);
    assert_eq!(source.samples.len(), SOURCE_FRAMES);
    let pcm_sha256 = f32_pcm_sha256(&source.samples);
    assert_eq!(pcm_sha256, text(&gate, "pcm_sha256"));
    // The complete immutable decoded source must have identical quarter blocks.
    assert!(
        source
            .samples
            .chunks_exact(LOOP_FRAMES)
            .all(|block| block == &source.samples[..LOOP_FRAMES])
    );
    Python::initialize();
    let engine = AudioEngine::new().unwrap();
    let mut mixer = RtMixer::new(1, RATE as f32);
    mixer.set_current_timing_acknowledgements(engine.current_timing_acknowledgements.clone());
    mixer.set_input_runtime_ownership(engine.input_runtime_ownership.clone());
    let (accepted_revision, accepted_epoch) =
        publish_actual_acceptance(&engine, &mut mixer, &source, &gate);
    let mut cases = Vec::new();
    for ratio in [1.0, 0.73, 1.25] {
        let mut pair = Vec::new();
        for partition_index in 0..PARTITIONS.len() {
            pair.push(render_case(
                &mut mixer,
                &source.samples,
                ratio,
                partition_index,
                &output_dir,
            ));
        }
        assert_eq!(
            pair[0]["actual_rendered_float32_le_sha256"],
            pair[1]["actual_rendered_float32_le_sha256"]
        );
        assert_eq!(
            pair[0]["actual_threshold_onsets"],
            pair[1]["actual_threshold_onsets"]
        );
        cases.extend(pair);
    }
    assert_eq!(file_sha256(&wav).unwrap(), source_sha256);
    let report = json!({
        "probe_revision": "private-g3c-actual-wav-dry-render-v1",
        "actual_original_path": wav, "actual_original_sha256": source_sha256,
        "native_loaded_pcm_sha256": pcm_sha256,
        "native_decode_output_rate_hz": RATE, "native_decode_output_channels": 1,
        "original_fixture_format": "48000Hz PCM24 mono600s; byte hash and retained G2 complete PCM identity verified",
        "native_loaded_frame_count": SOURCE_FRAMES, "native_loaded_duration_seconds": 600.0,
        "retained_gate_input": gate_path,
        "retained_gate_input_sha256": file_sha256(&gate_path).unwrap(),
        "retained_actual_beat_this_response": retained_response,
        "retained_actual_beat_this_response_sha256": retained_response_sha256,
        "retained_original_backend_job": gate["beat_this_request"]["identity"],
        "independent_quarter_provenance": gate["independent_quarters"]["provenance"],
        "fresh_accepted_revision": accepted_revision, "current_accepted_publication_epoch": accepted_epoch,
        "accepted_period_seconds_per_quarter": 0.5, "physical_loop_frames": LOOP_FRAMES,
        "physical_loop_duration_seconds": 0.5,
        "threshold_feature": {"absolute_amplitude_threshold": THRESHOLD, "refractory_output_frames": "floor(24000/rate/2)", "interpretation": "first threshold sample per isolated pulse; declared acoustic feature, no listening label", "expected_waveform": "independent closed form n*rate, integer modulo24000 and binary32 linear interpolation of actual decoded PCM"},
        "full_mix": true, "key_lock": false, "gain_db": 0.0, "eq_db": [0,0,0], "velocity": 1.0, "master_volume": 1.0,
        "partition_output_and_onset_equality": true, "cases": cases,
        "new_inference": false, "normal_preparation_default_acceptance": false,
        "retained_real_evidence_test_only_ticket_bridge": true,
        "normal_full600s_prepare_512MiB_peak_estimate_gate": "separate existing limitation; no production limit changed",
        "app_or_device_session": false, "human_listening_evidence": false,
        "copy_first_decoder_ABA_C1_proved": false, "audible_DSP_crop_delay_transition_B5_proved": false,
        "original_source_hash_after_matches": true,
    });
    fs::write(
        output_dir.join("summary.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
}

#[test]
#[ignore = "explicit hardware-free full-source budget probe; needs FLITZIS_G3C_WAV and FLITZIS_G3C_PCM_BUDGET_OUTPUT_DIR"]
fn private_actual_wav_pcm_budget_capture_prepare_publish_export() {
    let workspace = workspace();
    let wav = PathBuf::from(std::env::var_os("FLITZIS_G3C_WAV").unwrap())
        .canonicalize()
        .unwrap();
    assert!(wav.starts_with(workspace.join("test-audio")));
    let output_dir = PathBuf::from(std::env::var_os("FLITZIS_G3C_PCM_BUDGET_OUTPUT_DIR").unwrap());
    assert!(output_dir.is_absolute());
    let output_parent = output_dir.parent().unwrap().canonicalize().unwrap();
    assert!(output_parent.starts_with(workspace.join("scratch")));
    fs::create_dir(&output_dir).unwrap(); // A new receipt cannot overwrite an earlier attempt.
    let output_dir = output_dir.canonicalize().unwrap();
    assert!(output_dir.starts_with(workspace.join("scratch")));
    let original_sha256 = file_sha256(&wav).unwrap();
    assert_eq!(original_sha256, ORIGINAL_SHA256);
    let sample = decode_audio_file_to_sample_buffer(&wav, 2, RATE, |_| {}).unwrap();
    assert_eq!(sample.channels, 2);
    assert_eq!(sample.samples.len(), SOURCE_FRAMES * 2);
    let quarter_samples = LOOP_FRAMES * sample.channels;
    assert!(
        sample
            .samples
            .chunks_exact(quarter_samples)
            .all(|block| block == &sample.samples[..quarter_samples])
    );
    Python::initialize();
    let engine = AudioEngine::new().unwrap();
    let mut mixer = RtMixer::new(2, RATE as f32);
    mixer.set_current_timing_acknowledgements(engine.current_timing_acknowledgements.clone());
    mixer.set_input_runtime_ownership(engine.input_runtime_ownership.clone());
    mixer.set_prepared_source_epochs(engine.prepared_source_epochs.clone());
    let (producer, mut consumer) = super::tests::queue(2);
    engine.pad_request_ids.lock().unwrap()[0] = 7;
    engine.timing_intents.lock().unwrap()[0] = TimingIntent::Automatic;
    engine
        .input_runtime_ownership
        .set_timing_intent(0, TimingIntent::Automatic);
    let mut generations = engine.loaded_source_generations.lock().unwrap();
    let mut digests = engine.loaded_source_digests.lock().unwrap();
    publish_loaded_sample(
        &producer,
        &engine.sample_cache,
        0,
        sample.clone(),
        LoadedSourcePublication {
            cold: false,
            cold_epoch: None,
            cold_adoption: None,
            replace_assignment: false,
            loop_region: None,
            resident_cancelled: None,
            intent: None,
            ownership: &engine.input_runtime_ownership,
            generation: 7,
            rate: RATE,
            generation_slot: &mut generations[0],
            digest_slot: &mut digests[0],
            digest: original_sha256.clone(),
        },
    )
    .unwrap();
    drop(generations);
    drop(digests);
    let ControlMessage::LoadSample { id, sample: loaded } = consumer.pop().unwrap() else {
        panic!("native source publication must precede timing");
    };
    mixer.load_sample(id, loaded);
    let binding = crate::audio_engine::input_runtime_binding::capture(&engine, 0)
        .unwrap()
        .unwrap();
    let request_before = engine.pad_request_ids.lock().unwrap()[0];
    let epoch_before = engine.prepared_source_epochs[0].load(Ordering::Acquire);
    let timing_bound = || TimingBound {
        halfwidth_seconds: 0.05,
        provenance: "explicit authored-quarter fixture raw-QM feature matching bound".into(),
    };
    let default_error = capture_preparation(&engine, 0, timing_bound(), Some(&binding))
        .err()
        .unwrap();
    assert_eq!(default_error, "constant timing PCM byte limit exceeded");
    assert_eq!(engine.pad_request_ids.lock().unwrap()[0], request_before);
    assert_eq!(
        engine.prepared_source_epochs[0].load(Ordering::Acquire),
        epoch_before
    );
    assert!(binding.current());
    fs::write(
        output_dir.join("default-admission.json"),
        serde_json::to_vec_pretty(&json!({
            "evidence_kind":"hardware_free_actual_source_fixture",
            "actual_original_path":wav,
            "actual_original_sha256":original_sha256,
            "loaded_sample_rate_hz":RATE,
            "loaded_frame_count":SOURCE_FRAMES,
            "loaded_channels":2,
            "normal_pcm_limit_bytes":MAX_PCM_BYTES,
            "default_error":default_error,
            "request_before":request_before,
            "request_after":engine.pad_request_ids.lock().unwrap()[0],
            "epoch_before":epoch_before,
            "epoch_after":engine.prepared_source_epochs[0].load(Ordering::Acquire),
            "app_or_device_session":false,
            "device_acceptance":"pending",
            "human_listening":"pending"
        }))
        .unwrap(),
    )
    .unwrap();
    let explicit_limit = 1024 * 1024 * 1024;
    let captured =
        capture_preparation_with_limit(&engine, 0, timing_bound(), Some(&binding), explicit_limit)
            .unwrap();
    assert!(Arc::ptr_eq(&captured.sample.samples, &sample.samples));
    let ticket = prepare_captured(&engine, &captured).unwrap();
    assert_eq!(ticket.pcm_budget.limit_bytes(), explicit_limit);
    let actual_preparation: Value = Python::attach(|py| {
        let metadata = ticket.metadata(py).unwrap();
        let encoded: String = py
            .import("json")
            .unwrap()
            .call_method1("dumps", (metadata,))
            .unwrap()
            .extract()
            .unwrap();
        serde_json::from_str(&encoded).unwrap()
    });
    fs::write(
        output_dir.join("actual-native-preparation.json"),
        serde_json::to_vec_pretty(&actual_preparation).unwrap(),
    )
    .unwrap();
    let mut quarters = Vec::with_capacity(ticket.evidence.beat_seconds().len());
    let mut maximum_matching_error = 0.0_f64;
    for actual in ticket.evidence.beat_seconds() {
        assert!(actual.is_finite());
        let independent_quarter = (actual / 0.5).round() as i64;
        assert!((0..1200).contains(&independent_quarter));
        assert!(
            quarters
                .last()
                .is_none_or(|previous| *previous < independent_quarter)
        );
        let matching_error = (actual - independent_quarter as f64 * 0.5).abs();
        assert!(matching_error <= 0.05);
        maximum_matching_error = maximum_matching_error.max(matching_error);
        quarters.push(independent_quarter);
    }
    assert!(quarters.len() >= 3);
    let hypotheses = json!([{
        "id":"independently-authored-source-quarter-pulses",
        "provenance":"unchanged known source explicitly authors 1200 quarter pulses at n*0.5 seconds; actual raw events individually associated by independent source-time proximity",
        "verification":"verified",
        "quarter_note_denominator":1,
        "quarter_counts":quarters
    }])
    .to_string();
    publish(
        &engine,
        &producer,
        &ticket,
        &hypotheses,
        IndependentTimingOrigin {
            seconds: 0.0,
            provenance: "authored source quarter zero at original/loaded source frame zero".into(),
        },
        TimingAcceptanceDecision {
            policy_version: "hardware-free-full-source-pcm-budget-probe-v1".into(),
            provenance: "explicit diagnostic fixture units; no device or listening acceptance"
                .into(),
        },
    )
    .unwrap();
    assert_eq!(ticket.publication_status().unwrap(), "pending");
    assert!(
        export_current(&engine, 0, wav.to_string_lossy().into())
            .unwrap()
            .is_none()
    );
    assert!(super::tests::accept_message(
        &mut mixer,
        consumer.pop().unwrap()
    ));
    assert_eq!(ticket.publication_status().unwrap(), "accepted");
    let current: Value = Python::attach(|py| {
        let metadata = current_metadata(&engine, py, 0).unwrap().unwrap();
        let encoded: String = py
            .import("json")
            .unwrap()
            .call_method1("dumps", (metadata,))
            .unwrap()
            .extract()
            .unwrap();
        serde_json::from_str(&encoded).unwrap()
    });
    assert_eq!(current["pcm_limit_bytes"], explicit_limit);
    assert_eq!(current["source_sha256"], original_sha256);
    assert_eq!(current["frame_count"], SOURCE_FRAMES);
    assert_eq!(current["sample_rate_hz"], RATE);
    let revision = current["revision"].as_str().unwrap().to_owned();
    let period = current["period_seconds_per_quarter"].as_f64().unwrap();
    let pcm_sha256 = ticket.binding.pcm_sha256.clone();
    drop(ticket);
    drop(captured);
    let encoded = export_current(&engine, 0, wav.to_string_lossy().into())
        .unwrap()
        .unwrap();
    let exported: Value = serde_json::from_str(&encoded).unwrap();
    assert_eq!(exported["record"]["accepted_revision"], revision);
    assert_eq!(
        exported["record"]["period_bits"],
        format!("{:016x}", period.to_bits())
    );
    assert_eq!(
        exported["record"]["evidence"]["binding"]["pcm_sha256"],
        pcm_sha256
    );
    assert_eq!(current["pcm_sha256"], pcm_sha256);
    assert!(!encoded.contains("pcm_limit_bytes"));
    fs::write(
        output_dir.join("verified-current-timing.json"),
        encoded.as_bytes(),
    )
    .unwrap();
    assert_eq!(file_sha256(&wav).unwrap(), original_sha256);
    fs::write(
        output_dir.join("summary.json"),
        serde_json::to_vec_pretty(&json!({
            "schema_version":1,
            "evidence_kind":"hardware_free_actual_source_fixture",
            "probe_revision":"full-source-pcm-budget-capture-prepare-publish-export-v1",
            "actual_original_path":wav,
            "actual_original_sha256":original_sha256,
            "original_source_hash_after_matches":true,
            "loaded_sample_rate_hz":RATE,
            "loaded_frame_count":SOURCE_FRAMES,
            "loaded_channels":2,
            "normal_budget_rejected_before_request_advance":true,
            "explicit_pcm_limit_bytes":explicit_limit,
            "raw_qm_event_count":quarters.len(),
            "independent_source_quarter_counts":quarters,
            "maximum_raw_feature_matching_error_seconds":maximum_matching_error,
            "actual_preparation":"actual-native-preparation.json",
            "actual_preparation_sha256":file_sha256(&output_dir.join("actual-native-preparation.json")).unwrap(),
            "current_native_metadata":current,
            "current_full_accepted_revision":revision,
            "verified_export":"verified-current-timing.json",
            "verified_export_sha256":file_sha256(&output_dir.join("verified-current-timing.json")).unwrap(),
            "saved_runtime_budget_encoded":false,
            "app_or_device_session":false,
            "human_listening_evidence":false,
            "device_acceptance":"pending",
            "human_listening":"pending"
        }))
        .unwrap(),
    )
    .unwrap();
}
