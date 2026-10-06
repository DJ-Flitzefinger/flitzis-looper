//! Public lossless adapter contracts; no runtime publication or musical verification.

use flitzis_looper_analysis::tempo_evidence::{
    BackendEvidence, BeatThisModelIdentity, BeatThisRawEvidence, BeatThisRequestIdentity,
    BoundTempoEvidence, JobIdentity, MONO_REVISION, PcmBinding, PcmBindingMetadata,
    QmInputDescriptor, QmInputTransform, TimingBound, f32_pcm_sha256, f64_input_sha256,
};
use flitzis_looper_analysis::tempo_summary::{
    QuarterNoteHypothesis, QuarterNoteVerification, SummaryStatus, summarize_constant_tempo,
};
use flitzis_looper_analysis::{AnalysisConfig, analyze_bpm_raw};

fn metadata(samples: &[f32], rate: u32) -> PcmBindingMetadata {
    PcmBindingMetadata {
        job: JobIdentity {
            pad_id: 2,
            request_id: 42,
            source_id: "loaded-2-7".into(),
            source_generation: 7,
        },
        source_sha256: "1".repeat(64),
        source_provenance: "independently hashed original; fixture explicitly associated".into(),
        pcm_sha256: f32_pcm_sha256(samples),
        sample_rate_hz: rate,
        frame_count: samples.len() as u64,
        origin_seconds: 0.0,
        mono_revision: MONO_REVISION.into(),
    }
}

fn timing() -> TimingBound {
    TimingBound {
        halfwidth_seconds: 0.001,
        provenance: "declared engineering fixture bound; not calibrated musical uncertainty".into(),
    }
}

fn qm_input(
    binding: &PcmBinding<'_>,
    samples: &[f64],
    rate: u32,
    transform: QmInputTransform,
) -> QmInputDescriptor {
    QmInputDescriptor {
        job: binding.metadata().job.clone(),
        pcm_sha256: binding.metadata().pcm_sha256.clone(),
        input_sha256: f64_input_sha256(samples),
        sample_rate_hz: rate,
        frame_count: samples.len() as u64,
        origin_seconds: 0.0,
        transform,
    }
}

fn model() -> BeatThisModelIdentity {
    BeatThisModelIdentity {
        sha256: "a".repeat(64),
        frontend_id: "pinned-upstream-frontend".into(),
        environment_id: "verified-local-lock".into(),
        package_version: "1.1.0".into(),
        checkpoint: "final0".into(),
        postprocessor: "minimal".into(),
        device: "cpu".into(),
        precision: "float32".into(),
    }
}

fn beat_raw(binding: &PcmBinding<'_>) -> BeatThisRawEvidence {
    let expected_request = BeatThisRequestIdentity {
        job: binding.metadata().job.clone(),
        pcm_sha256: binding.metadata().pcm_sha256.clone(),
        pcm_path: "D:/fixture/retired-mono.f32le".into(),
        sample_rate_hz: binding.metadata().sample_rate_hz,
        frame_count: binding.metadata().frame_count,
        origin_seconds: 0.0,
        dtype: "float32-le".into(),
        channels: 1,
        schema_version: 1,
        model: model(),
    };
    BeatThisRawEvidence {
        response_job: expected_request.job.clone(),
        response_model: expected_request.model.clone(),
        response_schema_version: 1,
        expected_request,
        beat_seconds: vec![-0.0, 1.0000000000000002, 2.1234567890123457],
        downbeat_seconds: vec![0.125, 1.25],
        beat_logits: vec![-0.0, f64::from_bits(1), -f64::from_bits(1), f64::MAX],
        downbeat_logits: vec![0.12345678901234568, 1.0000000000000002, -2.5, -f64::MAX],
    }
}

fn assert_bits(left: &[f64], right: &[f64]) {
    assert_eq!(left.len(), right.len());
    for (left, right) in left.iter().zip(right) {
        assert_eq!(left.to_bits(), right.to_bits());
    }
}

#[test]
fn pcm_and_input_hashes_use_exact_little_endian_bits() {
    // Independent Python hashlib/struct byte oracle, including positive/negative zero.
    let pcm = [0.0_f32, -0.0, 1.0, -2.5];
    let input = [0.0_f64, -0.0, 1.0, -2.5];
    assert_eq!(
        f32_pcm_sha256(&pcm),
        "283e4f49b9351bde5277c7018f4a353063da06644e860cfbaba79fea476349ec"
    );
    assert_eq!(
        f64_input_sha256(&input),
        "b54a1eb588aeaa3bf8c7d82e85010d5bdcee3aeca42a1e4a7913d005353dd48d"
    );
    let positive_zero = [0.0_f32; 4];
    assert_ne!(f32_pcm_sha256(&positive_zero), f32_pcm_sha256(&pcm));
    let binding = PcmBinding::verify(&pcm, metadata(&pcm, 48_000)).unwrap();
    assert_eq!(binding.samples().as_ptr(), pcm.as_ptr());
    assert_eq!(binding.samples()[1].to_bits(), (-0.0_f32).to_bits());
}

#[test]
fn binding_rejects_wrong_content_metadata_and_missing_original_provenance() {
    let pcm = vec![0.0_f32; 64];
    for case in 0..11 {
        let mut meta = metadata(&pcm, 48_000);
        match case {
            0 => meta.pcm_sha256 = "2".repeat(64),
            1 => meta.frame_count += 1,
            2 => meta.sample_rate_hz = 0,
            3 => meta.origin_seconds = 1.0 / 48_000.0,
            4 => meta.origin_seconds = -0.0,
            5 => meta.source_sha256 = "not-a-content-hash".into(),
            6 => meta.source_provenance = " ".into(),
            7 => meta.job.request_id = 0,
            8 => meta.job.source_generation = 0,
            9 => meta.job.source_id.clear(),
            _ => meta.mono_revision = "unidentified-channel-mix".into(),
        }
        assert!(PcmBinding::verify(&pcm, meta).is_err(), "case {case}");
    }
    let mut nonfinite = pcm.clone();
    nonfinite[63] = f32::INFINITY;
    assert!(PcmBinding::verify(&nonfinite, metadata(&nonfinite, 48_000)).is_err());
    assert!(PcmBinding::verify(&[], metadata(&[], 48_000)).is_err());
}

#[test]
fn qm_adapter_keeps_actual_timebase_raw_frames_indices_and_requested_config() {
    let rate = 48_000;
    let mut pcm = vec![0.0_f32; rate as usize * 15 + 19];
    for start in (0..pcm.len() - 100).step_by(rate as usize / 2) {
        for offset in 0..64 {
            pcm[start + offset] = 1.0 - offset as f32 / 64.0;
        }
    }
    let input: Vec<f64> = pcm.iter().map(|sample| f64::from(*sample)).collect();
    let config = AnalysisConfig {
        step_secs: 0.01031,
        max_bin_hz: 60.0,
        input_tempo: 123.75,
        alpha: 0.87,
        tightness: 3.9,
        viterbi_sigma: 5.25,
        window_length: 321,
        hop_size: 65,
    };
    let raw = analyze_bpm_raw(&input, rate, &config).unwrap();
    assert!(!raw.beat_frames().is_empty());
    let frames = raw.beat_frames().to_vec();
    let downbeat_indices = raw.downbeat_raw_indices().to_vec();
    let seconds: Vec<f64> = raw.beat_seconds().collect();
    let binding = PcmBinding::verify(&pcm, metadata(&pcm, rate)).unwrap();
    let descriptor = qm_input(&binding, &input, rate, QmInputTransform::Identity);
    let evidence =
        BoundTempoEvidence::from_qm(&binding, raw, &input, descriptor.clone(), timing(), -0.375)
            .unwrap();
    assert_bits(evidence.beat_seconds(), &seconds);
    let BackendEvidence::Qm { raw, input } = evidence.backend() else {
        panic!("QM capture replaced by another backend");
    };
    assert_bits(raw.beat_frames(), &frames);
    assert_eq!(raw.downbeat_raw_indices(), downbeat_indices);
    assert_eq!(raw.configuration(), &config);
    assert_eq!(raw.odf_hop_samples(), 494);
    assert_eq!(input.as_ref(), &descriptor);
    assert_eq!(evidence.raw_evidence().independent_origin_seconds, -0.375);
    assert_eq!(
        evidence.source_identity().loaded_frame_count,
        pcm.len() as u64
    );
    assert_eq!(evidence.source_identity().loaded_sample_rate_hz, rate);
    assert_eq!(evidence.timing_bound(), &timing());
}

#[test]
fn qm_adapter_rejects_stale_identity_wrong_input_and_false_identity_transform() {
    let pcm = vec![0.0_f32; 128];
    let input = vec![0.0_f64; 128];
    let binding = PcmBinding::verify(&pcm, metadata(&pcm, 48_000)).unwrap();
    let raw = analyze_bpm_raw(&input, 48_000, &AnalysisConfig::default()).unwrap();
    for case in 0..9 {
        let mut descriptor = qm_input(&binding, &input, 48_000, QmInputTransform::Identity);
        let mut actual = input.clone();
        match case {
            0 => descriptor.job.request_id += 1,
            1 => descriptor.job.source_generation += 1,
            2 => descriptor.pcm_sha256 = "2".repeat(64),
            3 => descriptor.input_sha256 = "3".repeat(64),
            4 => descriptor.frame_count -= 1,
            5 => descriptor.sample_rate_hz = 44_100,
            6 => descriptor.origin_seconds = 0.001,
            7 => {
                actual[17] = 0.25;
                descriptor.input_sha256 = f64_input_sha256(&actual);
            }
            _ => {
                actual[0] = -0.0;
                descriptor.input_sha256 = f64_input_sha256(&actual);
            }
        }
        assert!(
            BoundTempoEvidence::from_qm(&binding, raw.clone(), &actual, descriptor, timing(), 0.0,)
                .is_err(),
            "case {case}"
        );
    }
}

#[test]
fn qm_standard_transform_records_distinct_loaded_and_analyzer_domains() {
    for (rate, frames) in [(48_000, 4703_usize), (96_000, 4703), (48_001, 4717)] {
        let pcm = vec![0.0_f32; frames];
        let analyzer_frames = (frames as u128 * 44_100).div_ceil(u128::from(rate)) as usize;
        let input = vec![0.0_f64; analyzer_frames];
        let binding = PcmBinding::verify(&pcm, metadata(&pcm, rate)).unwrap();
        let raw = analyze_bpm_raw(&input, 44_100, &AnalysisConfig::default()).unwrap();
        let transform = QmInputTransform::Rubato44100 {
            revision: "rubato-fft-1.0-44100-delay-trim-tail-flush-v1".into(),
            provenance: "same immutable job export transformed with pinned standard converter"
                .into(),
        };
        let descriptor = qm_input(&binding, &input, 44_100, transform);
        let evidence =
            BoundTempoEvidence::from_qm(&binding, raw, &input, descriptor, timing(), 0.0).unwrap();
        assert_eq!(evidence.binding().sample_rate_hz, rate);
        assert_eq!(evidence.binding().frame_count, frames as u64);
        let BackendEvidence::Qm { raw, .. } = evidence.backend() else {
            panic!("wrong backend");
        };
        assert_eq!(raw.input_sample_rate_hz(), 44_100);
        assert_eq!(raw.input_frame_count(), analyzer_frames);
    }
}

#[test]
fn qm_standard_transform_rejects_wrong_tail_extent_revision_and_provenance() {
    let pcm = vec![0.0_f32; 4703];
    let binding = PcmBinding::verify(&pcm, metadata(&pcm, 96_000)).unwrap();
    let frames = (4703_u128 * 44_100).div_ceil(96_000) as usize;
    for case in 0..4 {
        let input = vec![0.0_f64; frames + usize::from(case == 0)];
        let raw = analyze_bpm_raw(&input, 44_100, &AnalysisConfig::default()).unwrap();
        let transform = QmInputTransform::Rubato44100 {
            revision: if case == 1 {
                "unidentified-resampler".into()
            } else {
                "rubato-fft-1.0-44100-delay-trim-tail-flush-v1".into()
            },
            provenance: if case == 2 {
                "".into()
            } else {
                "explicit transformer assertion".into()
            },
        };
        let mut descriptor = qm_input(&binding, &input, 44_100, transform);
        if case == 3 {
            descriptor.sample_rate_hz = 48_000;
        }
        assert!(
            BoundTempoEvidence::from_qm(&binding, raw, &input, descriptor, timing(), 0.0).is_err(),
            "case {case}"
        );
    }
}

#[test]
fn beat_this_adapter_preserves_all_binary64_arrays_full_metadata_and_long_positions() {
    let pcm = vec![0.0_f32; 8_000 * 600];
    let binding = PcmBinding::verify(&pcm, metadata(&pcm, 8_000)).unwrap();
    let mut raw = beat_raw(&binding);
    raw.beat_seconds
        .push(f64::from_bits(599.5_f64.to_bits() + 1));
    raw.downbeat_seconds.push(599.0);
    let expected = raw.clone();
    let evidence = BoundTempoEvidence::from_beat_this(&binding, raw, timing(), -0.75).unwrap();
    drop(binding);
    drop(pcm);
    let BackendEvidence::BeatThis(retained) = evidence.backend() else {
        panic!("worker evidence replaced by another backend");
    };
    assert_bits(&retained.beat_seconds, &expected.beat_seconds);
    assert_bits(&retained.downbeat_seconds, &expected.downbeat_seconds);
    assert_bits(&retained.beat_logits, &expected.beat_logits);
    assert_bits(&retained.downbeat_logits, &expected.downbeat_logits);
    assert_bits(evidence.beat_seconds(), &expected.beat_seconds);
    assert_eq!(retained.expected_request, expected.expected_request);
    assert_eq!(retained.response_job, expected.response_job);
    assert_eq!(retained.response_model, expected.response_model);
    assert_eq!(evidence.raw_evidence().independent_origin_seconds, -0.75);
    assert_eq!(
        evidence.binding().source_provenance,
        "independently hashed original; fixture explicitly associated"
    );
    assert_ne!(
        evidence.beat_seconds()[3].to_bits(),
        f64::from(evidence.beat_seconds()[3] as f32).to_bits()
    );
}

#[test]
fn beat_this_adapter_rejects_source_request_model_timebase_and_array_mismatches() {
    let pcm = vec![0.0_f32; 8_000 * 4];
    let binding = PcmBinding::verify(&pcm, metadata(&pcm, 8_000)).unwrap();
    for case in 0..19 {
        let mut raw = beat_raw(&binding);
        match case {
            0 => raw.expected_request.job.request_id += 1,
            1 => raw.expected_request.job.source_generation += 1,
            2 => raw.expected_request.pcm_sha256 = "2".repeat(64),
            3 => raw.expected_request.sample_rate_hz = 16_000,
            4 => raw.expected_request.frame_count -= 1,
            5 => raw.expected_request.origin_seconds = 1.0,
            6 => raw.expected_request.dtype = "float64-le".into(),
            7 => raw.expected_request.channels = 2,
            8 => raw.expected_request.schema_version = 2,
            9 => raw.expected_request.pcm_path.clear(),
            10 => raw.response_job.request_id += 1,
            11 => raw.response_model.sha256 = "b".repeat(64),
            12 => raw.response_schema_version = 2,
            13 => {
                raw.expected_request.model.precision = "float64".into();
                raw.response_model = raw.expected_request.model.clone();
            }
            14 => raw.beat_seconds[1] = raw.beat_seconds[0],
            15 => raw.downbeat_seconds.push(4.0),
            16 => raw.beat_logits[0] = f64::NAN,
            17 => {
                raw.downbeat_logits.pop();
            }
            _ => raw.beat_logits.resize(250_001, 0.0),
        }
        assert!(
            BoundTempoEvidence::from_beat_this(&binding, raw, timing(), 0.0).is_err(),
            "case {case}"
        );
    }
}

#[test]
fn canonical_raw_identity_covers_every_worker_array_metadata_and_floating_bit() {
    let pcm = vec![0.0_f32; 8_000 * 4];
    let binding = PcmBinding::verify(&pcm, metadata(&pcm, 8_000)).unwrap();
    let original = beat_raw(&binding);
    let evidence =
        BoundTempoEvidence::from_beat_this(&binding, original.clone(), timing(), 0.0).unwrap();
    let repeated =
        BoundTempoEvidence::from_beat_this(&binding, original.clone(), timing(), 0.0).unwrap();
    assert_eq!(
        evidence.source_identity().raw_revision,
        repeated.source_identity().raw_revision
    );
    for case in 0..7 {
        let mut raw = original.clone();
        match case {
            0 => raw.beat_seconds[0] = 0.0,
            1 => raw.downbeat_seconds[0] += 0.001,
            2 => raw.beat_logits[0] = 0.0,
            3 => raw.downbeat_logits[0] += 0.0001,
            4 => raw.expected_request.pcm_path = "D:/another-retired-export.f32le".into(),
            5 => {
                raw.expected_request.model.environment_id += "/another-lock";
                raw.response_model = raw.expected_request.model.clone();
            }
            _ => {
                raw.expected_request.model.frontend_id += "/another-frontend";
                raw.response_model = raw.expected_request.model.clone();
            }
        }
        let changed = BoundTempoEvidence::from_beat_this(&binding, raw, timing(), 0.0).unwrap();
        assert_ne!(
            evidence.source_identity().raw_revision,
            changed.source_identity().raw_revision,
            "case {case}"
        );
    }
}

#[test]
fn requested_qm_settings_change_raw_identity_even_when_legacy_tracker_ignores_them() {
    let pcm = vec![0.0_f32; 128];
    let input = vec![0.0_f64; 128];
    let binding = PcmBinding::verify(&pcm, metadata(&pcm, 48_000)).unwrap();
    let descriptor = qm_input(&binding, &input, 48_000, QmInputTransform::Identity);
    let mut revisions = Vec::new();
    for case in 0..4 {
        let mut config = AnalysisConfig::default();
        match case {
            0 => {}
            1 => config.viterbi_sigma += 1.0,
            2 => config.window_length += 1,
            _ => config.hop_size += 1,
        }
        let raw = analyze_bpm_raw(&input, 48_000, &config).unwrap();
        let evidence =
            BoundTempoEvidence::from_qm(&binding, raw, &input, descriptor.clone(), timing(), 0.0)
                .unwrap();
        let revision = evidence.source_identity().raw_revision.clone();
        assert!(!revisions.contains(&revision));
        revisions.push(revision);
    }
}

#[test]
fn adapter_bound_is_explicit_and_numerical_fit_cannot_verify_quarter_notes() {
    let pcm = vec![0.0_f32; 8_000 * 40];
    let binding = PcmBinding::verify(&pcm, metadata(&pcm, 8_000)).unwrap();
    let mut raw = beat_raw(&binding);
    raw.beat_seconds = (0..80).map(|index| index as f64 * 0.5).collect();
    let evidence =
        BoundTempoEvidence::from_beat_this(&binding, raw.clone(), timing(), -0.25).unwrap();
    let counts: Vec<Option<i64>> = (0..80).map(Some).collect();
    let hypothesis = QuarterNoteHypothesis {
        id: "detector-sequence-proposal",
        provenance: "model predictions alone, no independent quarter-note evidence",
        verification: QuarterNoteVerification::Unverified,
        quarter_note_denominator: 1,
        quarter_counts: &counts,
    };
    let summary = summarize_constant_tempo(&evidence.raw_evidence(), &[hypothesis]).unwrap();
    assert_eq!(summary.status, SummaryStatus::Unverified);
    assert_eq!(summary.independent_origin_seconds, -0.25);
    for invalid in [
        TimingBound {
            halfwidth_seconds: f64::NAN,
            provenance: "caller".into(),
        },
        TimingBound {
            halfwidth_seconds: -0.001,
            provenance: "caller".into(),
        },
        TimingBound {
            halfwidth_seconds: 41.0,
            provenance: "caller".into(),
        },
        TimingBound {
            halfwidth_seconds: 0.001,
            provenance: "".into(),
        },
    ] {
        assert!(BoundTempoEvidence::from_beat_this(&binding, raw.clone(), invalid, 0.0).is_err());
    }
    assert!(BoundTempoEvidence::from_beat_this(&binding, raw, timing(), f64::INFINITY).is_err());
}
