//! C2b finite storage through the actual complete-evidence/native ACK route.
//! Expected PCM and musical boundaries use the independent rational fixture
//! arithmetic in the parent; no reader/clock helper supplies expected values.

use super::*;
use crate::audio_engine::audio_stream::drain_control_messages;
use crate::audio_engine::buffer_retirement::create_audio_buffer_retirement;
use crate::audio_engine::input_runtime_binding;
use crate::audio_engine::prepared_source::PreparedSourcePermit;
use crate::audio_engine::scheduler::FixedCapacityScheduler;
use crate::audio_engine::transport::TransportTimeline;
use crate::messages::{ResidentContext, ResidentTransaction, TriggerQuantization};
use std::time::{Duration, Instant};

fn independent_pcm_sha256(samples: &[f32]) -> String {
    let mut hash = Sha256::new();
    for sample in samples {
        hash.update(sample.to_le_bytes());
    }
    format!("{:x}", hash.finalize())
}

fn render_resident(case: Case, partition: &[usize]) -> (Vec<f32>, serde_json::Value) {
    let (engine, ticket, mut mixer) = accepted_mixer(case);
    let accepted = ticket.guard.lock().unwrap().accepted().unwrap().clone();
    let evidence_revision = accepted.revision().to_owned();
    let complete = ticket.sample.clone();
    let complete_hash = independent_pcm_sha256(&complete.samples);
    assert_eq!(complete_hash, ticket.binding.pcm_sha256);
    assert_eq!(complete_hash, ticket.source_digest);
    let complete_backing = Arc::downgrade(&complete.samples);
    let finite = complete
        .window(
            case.physical_start(),
            case.physical_end(),
            2,
            ResidentContext::FiniteLoop,
        )
        .unwrap();
    let window_hash = independent_pcm_sha256(&finite.samples);
    let raw_revision = accepted.evidence().source_identity().raw_revision.clone();
    let processing_identity = complete.residency.as_ref().unwrap().source.transform_sha256;
    let source_address = complete.source_address();
    let complete_frames = complete.frame_count();
    let captured = input_runtime_binding::capture(&engine, 0).unwrap().unwrap();
    assert!(captured.current());
    let publication = PreparedSourcePermit::new(
        engine.prepared_source_epochs[0].clone(),
        engine.prepared_source_epochs[0].load(Ordering::Acquire),
    );
    publication.mark_pending().unwrap();
    let (mut producer, mut consumer) = rtrb::RingBuffer::new(2);
    producer
        .push(ControlMessage::RelocateResident(Box::new(
            ResidentTransaction {
                id: 0,
                sample: finite.clone(),
                stems: None,
                binding: captured.binding,
                publication: publication.clone(),
                expected_window_revision: complete.window_revision(),
                intent: Default::default(),
                seek_pin: None,
            },
        )))
        .unwrap();
    let (mut retirement, _worker) = create_audio_buffer_retirement();
    assert_eq!(publication.status(), "pending");
    assert_eq!(mixer.voices[0].sample.as_ref().unwrap().resident_start(), 0);
    assert_eq!(
        drain_control_messages(
            &mut consumer,
            &mut FixedCapacityScheduler::<8>::new(),
            0,
            &mut TriggerQuantization::Immediate,
            &mut TransportTimeline::new(case.sample_rate),
            &mut mixer,
            &mut Vec::new(),
            &mut retirement,
        ),
        1
    );
    assert_eq!(publication.status(), "accepted");
    assert!(!captured.current());
    assert_eq!(
        mixer.voices[0].source_timing.accepted.unwrap().revision,
        captured.binding.accepted.unwrap().revision
    );
    assert_eq!(
        mixer.voices[0].sample.as_ref().unwrap().source_address(),
        source_address
    );
    assert_eq!(
        mixer.voices[0].sample.as_ref().unwrap().frame_count(),
        complete_frames
    );
    *engine.sample_cache.lock().unwrap().get_mut(0).unwrap() = Some(finite.clone());
    drop(ticket);
    drop(complete);
    let deadline = Instant::now() + Duration::from_secs(5);
    while complete_backing.upgrade().is_some() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        complete_backing.upgrade().is_none(),
        "finite reader retained complete PCM"
    );
    let admitted_period = accepted.period_seconds_per_quarter()
        * f64::from(case.sample_rate)
        * case.loop_ticks as f64
        / TICKS_PER_QUARTER as f64;
    let (numerator, denominator) = case.musical_loop();
    let declared_period = numerator as f64 / denominator as f64;
    let rate = case.ratio();
    let frames = case.rendered_frames(1000);
    let mut output = vec![0.0; frames];
    let mut offset = 0;
    let mut callback = 0;
    let mut previous_phase = 0.0;
    let mut wraps = 0;
    let mut checkpoints = Vec::new();
    while offset < frames {
        let count = partition[callback % partition.len()].min(frames - offset);
        mixer.render_rt_at_output_frame(
            &mut output[offset..offset + count],
            &mut [0.0; crate::audio_engine::constants::NUM_SAMPLES],
            offset as u64,
            &mut retirement,
        );
        offset += count;
        callback += 1;
        let position = mixer.voices[0].source_playback.position();
        let phase = position.frame as f64 + position.fraction - case.physical_start() as f64;
        if partition == [1] && phase < previous_phase {
            wraps += 1;
            if [75, 1000].contains(&wraps) {
                let boundary = offset as f64 * rate - phase;
                let error = boundary - wraps as f64 * declared_period;
                let admitted_error = boundary - wraps as f64 * admitted_period;
                assert!(error.abs() <= 1.0, "loaded-frame boundary {wraps}: {error}");
                assert!(admitted_error.abs() <= 1.0);
                checkpoints.push(json!({
                    "observed_native_wrap":wraps,"output_frames":offset,
                    "actual_phase_loaded_frames":phase,"actual_boundary_loaded_frames":boundary,
                    "independent_declared_boundary_loaded_frames":wraps as f64*declared_period,
                    "declared_error_loaded_frames":error,"admitted_error_loaded_frames":admitted_error
                }));
            }
        }
        previous_phase = phase;
    }
    if partition == [1] {
        assert_eq!(wraps, 1000);
        assert_eq!(checkpoints.len(), 2);
    }
    let mut max_error = 0.0_f32;
    for (frame, actual) in output.iter().enumerate() {
        let phase = (frame as f64 * rate).rem_euclid(admitted_period);
        let expected = independent_plateau_sample(phase, case.physical_length(), admitted_period);
        max_error = max_error.max((actual - expected).abs());
    }
    assert!(max_error <= 1e-7, "independent PCM error {max_error}");
    let onsets: Vec<_> = output
        .iter()
        .enumerate()
        .filter_map(|(frame, sample)| {
            (*sample > THRESHOLD && (frame == 0 || output[frame - 1] <= THRESHOLD)).then_some(frame)
        })
        .collect();
    assert_eq!(onsets.len(), 1001);
    let row = json!({
        "sample_rate_hz":case.sample_rate,"physical_H":case.physical_length(),
        "declared_musical_P":declared_period,"admitted_musical_P":admitted_period,
        "rate":rate,"partition":partition,"output_frames":frames,
        "complete_pcm_sha256":complete_hash,"window_pcm_sha256":window_hash,
        "render_pcm_sha256":independent_pcm_sha256(&output),"accepted_complete_evidence_revision":evidence_revision,
        "complete_raw_evidence_revision":raw_revision,"processing_identity_sha256_bytes":processing_identity,
        "full_frame_count":complete_frames,"resident_start":finite.resident_start(),
        "resident_end":finite.resident_end(),"window_revision":finite.window_revision(),
        "source_zero_frame":finite.residency.as_ref().unwrap().source.source_zero_frame,
        "native_ACK":publication.status(),"full_pcm_pin_released":complete_backing.upgrade().is_none(),
        "independent_max_sample_error":max_error,"native_observed_wraps":if partition==[1]{Some(wraps)}else{None},
        "observed_75_1000_checkpoints":checkpoints,"actual_onsets":onsets,
        "scope":"generated complete evidence and actual native drain/render; no inference, process RAM, device or listening claim"
    });
    (output, row)
}

#[test]
fn actual_complete_accepted_resident_output_matches_fullbuffer_and_independent_oracle_75_1000_cycles()
 {
    let mut rows = Vec::new();
    for sample_rate in [44_100, 48_000, 96_000] {
        for (quarter_numerator, quarter_denominator, label) in [
            (32_769, 64, "resident-P-greater-than-H"),
            (33_047, 64, "resident-P-less-than-H"),
            (512, 1, "resident-P-equals-H"),
        ] {
            for (rate_numerator, rate_denominator) in [(73, 100), (1, 1), (5, 4)] {
                let case = Case {
                    sample_rate,
                    quarter_numerator,
                    quarter_denominator,
                    rate_numerator,
                    rate_denominator,
                    loop_ticks: 1,
                    origin_numerator: -17,
                    origin_denominator: 4,
                    label,
                };
                let (complete_output, full_row) = render(case, &[512], 1000);
                for partition in [&[512][..], &[1, 17, 257, 64, 1023, 3][..], &[1][..]] {
                    let (resident_output, mut row) = render_resident(case, partition);
                    assert_eq!(
                        resident_output,
                        complete_output,
                        "{label} Fs={sample_rate} rate={} partition={partition:?}",
                        case.ratio()
                    );
                    assert_eq!(row["complete_pcm_sha256"], full_row["pcm_sha256"]);
                    assert_eq!(
                        row["accepted_complete_evidence_revision"],
                        full_row["accepted_revision"]
                    );
                    row["fullbuffer_render_pcm_sha256"] =
                        json!(independent_pcm_sha256(&complete_output));
                    row["independent_fullbuffer_checkpoints"] = full_row["checkpoints"].clone();
                    rows.push(row);
                }
            }
        }
    }
    export("c2b-resident-complete-75-1000-proof.json", &rows);
}
