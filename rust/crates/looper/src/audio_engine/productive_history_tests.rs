//! Productive native/FIFO/filter ownership and chronological source continuity.

use super::*;
use crate::audio_engine::prepared_source::PreparedSourcePermit;
use crate::audio_engine::productive_source_history::ProductiveSourceHistory;
use flitzis_looper_analysis::tempo_acceptance::TimingIntent;
use std::sync::atomic::AtomicU64;

const RATE: u32 = 8_000;

#[test]
fn fractional_musical_domain_retains_pinned_timing_query_pause_and_filter_history() {
    let original = sample(0.5);
    let replacement = sample(0.2);
    let mut mixer = setup(&original);
    let accepted = AcceptedTimingProjection {
        period_seconds: 250.25 * 16.0 / f64::from(RATE),
        ..timing(11, 2, -0.031_25)
    };
    let expected_period = accepted.period_seconds * f64::from(RATE) / 16.0;
    assert!((expected_period - 250.25).abs() < 1.0e-12);
    assert!(adopt(&mut mixer, &original, accepted));
    mixer.set_pad_loop_region(0, 80.0 / f64::from(RATE), Some(330.0 / f64::from(RATE)));
    mixer.set_bpm_lock(true);
    mixer.set_master_period(accepted.period_seconds / 1.37);
    render(&mut mixer, 1657);
    let native = voice(&mixer).stretch.native_state_address();
    let before = history(&mixer);
    let phase = voice(&mixer).source_playback.position();
    assert_eq!(
        voice(&mixer).source_playback.loop_period(),
        Some(expected_period)
    );
    let expected_beat =
        (phase.frame as f64 + phase.fraction + 250.0) / (accepted.period_seconds * f64::from(RATE));
    assert_eq!(mixer.active_pad_beat_position(0), Some(expected_beat));
    mixer.pause_sample(0);
    assert!(render(&mut mixer, 127).iter().all(|value| *value == 0.0));
    assert_eq!(voice(&mixer).source_playback.position(), phase);
    assert_eq!(
        voice(&mixer).source_playback.loop_period(),
        Some(expected_period)
    );
    mixer.resume_sample(0);
    assert!(mixer.load_sample_rt(0, replacement.clone(), &mut ImmediateAudioBufferRetirement));
    let next = AcceptedTimingProjection {
        period_seconds: 0.75,
        ..timing(12, 3, 0.071)
    };
    assert!(adopt(&mut mixer, &replacement, next));
    render(&mut mixer, 777);
    assert_eq!(
        voice(&mixer).source_playback.loop_period(),
        Some(expected_period)
    );
    assert_eq!(voice(&mixer).source_timing.accepted, Some(accepted));
    assert_eq!(voice(&mixer).stretch.native_state_address(), native);
    let continued = history(&mixer);
    assert_eq!(continued.binding.accepted, Some(accepted));
    assert_eq!(continued.fed_output_frames, before.fed_output_frames + 777);
    let filtered = mixer.pad_dsp_chains[0].source_history().unwrap();
    assert_eq!(filtered.fed_output_frames, before.fed_output_frames + 777);
    assert_eq!(
        filtered.next_position,
        voice(&mixer).source_playback.position()
    );
    let distance = phase.frame as f64 - 80.0 + phase.fraction + 777.0 * 1.37;
    let expected = distance.rem_euclid(expected_period);
    let actual = voice(&mixer).source_playback.position();
    assert!((actual.frame as f64 - 80.0 + actual.fraction - expected).abs() < 1.0e-9);
    assert_eq!(mixer.loop_region_frames(0), (80, Some(330)));
}

#[test]
fn productive_history_seek_uses_retained_voice_extent_and_clears_fixed_history_before_same_phase_seek()
 {
    finite_productive_seek_cases();
    let original = sample(0.5);
    let shorter = SampleBuffer {
        residency: None,
        channels: 1,
        samples: Arc::from(vec![0.2; RATE as usize / 4]),
    };
    let mut mixer = Box::new(setup(&original));
    let accepted = timing(1, 2, -0.0);
    assert!(adopt(&mut mixer, &original, accepted));
    render(&mut mixer, 1657);
    mixer.load_sample(0, shorter);
    assert!(mixer.seek_sample(0, 1.5));
    assert_eq!(voice(&mixer).frame_pos, 12_000); // old pinned extent; replacement has only 2,000
    assert_eq!(voice(&mixer).source_timing.accepted, Some(accepted));
    assert!(voice(&mixer).stretch.productive_history().is_none());
    assert_eq!(voice(&mixer).stretch.pending_fifo_frames(), (0, 0));
    assert!(mixer.pad_dsp_chains[0].source_history().is_none());
    assert!(Arc::ptr_eq(
        &voice(&mixer).sample.as_ref().unwrap().samples,
        &original.samples
    ));
    mixer.set_pad_key_lock(0, false);
    assert_eq!(render(&mut mixer, 1)[0], original.samples[12_000]);

    let mut same_phase = Box::new(setup(&original));
    same_phase.set_speed(1.5);
    assert!(same_phase.play_sample(0, 1.0));
    render(&mut same_phase, 1024);
    assert_eq!(voice(&same_phase).source_playback.position().fraction, 0.0);
    let position = voice(&same_phase).source_playback.position();
    assert!(same_phase.pad_dsp_chains[0].source_history().is_some());
    assert!(voice(&same_phase).stretch.productive_history().is_some());
    assert!(same_phase.seek_sample(0, position.frame as f64 / f64::from(RATE)));
    assert_eq!(voice(&same_phase).source_playback.position(), position);
    assert!(same_phase.pad_dsp_chains[0].source_history().is_none());
    assert!(voice(&same_phase).stretch.productive_history().is_none());
    assert_eq!(voice(&same_phase).stretch.pending_fifo_frames(), (0, 0));
}

fn sample(value: f32) -> SampleBuffer {
    SampleBuffer {
        residency: None,
        channels: 1,
        samples: Arc::from(
            (0..RATE * 2)
                .map(|frame| value * (frame as f32 * 0.13).sin())
                .collect::<Vec<_>>(),
        ),
    }
}

fn timing(revision: u8, epoch: u64, origin: f64) -> AcceptedTimingProjection {
    AcceptedTimingProjection {
        revision: [revision; 32],
        period_seconds: 0.500_123_456_789_012_3,
        origin_seconds: origin,
        sample_rate_hz: RATE,
        publication_epoch: epoch,
    }
}

fn adopt(mixer: &mut RtMixer, source: &SampleBuffer, projection: AcceptedTimingProjection) -> bool {
    let permit = PreparedSourcePermit::for_epoch(
        Arc::new(AtomicU64::new(projection.publication_epoch)),
        projection.publication_epoch,
    );
    permit.mark_pending().unwrap();
    mixer.publish_constant_timing_rt(
        0,
        PreparedConstantTiming {
            reference: source.clone(),
            publication: permit,
            projection,
        },
        &mut ImmediateAudioBufferRetirement,
    )
}

fn setup(source: &SampleBuffer) -> RtMixer {
    let mut mixer = RtMixer::new(1, RATE as f32);
    mixer.load_sample(0, source.clone());
    mixer.set_pad_bpm(0, Some(120.0));
    mixer.set_speed(1.371_234_567_890_123);
    mixer.set_pad_key_lock(0, true);
    assert!(mixer.play_sample(0, 1.0));
    mixer
}

fn render(mixer: &mut RtMixer, frames: usize) -> Vec<f32> {
    let mut output = vec![0.0; frames];
    mixer.render(&mut output, &mut [0.0; NUM_SAMPLES]);
    output
}

fn voice(mixer: &RtMixer) -> &VoiceSlot {
    mixer
        .voices
        .iter()
        .find(|voice| voice.is_playing_sample(0))
        .unwrap()
}

fn history(mixer: &RtMixer) -> ProductiveSourceHistory {
    voice(mixer).stretch.productive_history().unwrap()
}

#[test]
fn productive_history_actual_native_and_pending_fifos_survive_full_same_source_revision_refresh() {
    let source = sample(0.7);
    let mut actual = setup(&source);
    let mut reference = setup(&source);
    assert!(voice(&actual).stretch.productive_history().is_none());
    let first = timing(1, 2, 0.0);
    assert!(adopt(&mut actual, &source, first));
    assert!(adopt(&mut reference, &source, first));
    for frames in [512, 512, 512, 127] {
        assert_eq!(render(&mut actual, frames), render(&mut reference, frames));
    }
    let native = voice(&actual).stretch.native_state_address();
    let fifo = voice(&actual).stretch.pending_fifo_frames();
    assert!(fifo.0 > 0 && fifo.1 > 0);
    let before = history(&actual);
    assert_eq!(
        before.binding.source_address,
        source.samples.as_ptr() as usize
    );
    assert_eq!(before.binding.accepted, Some(first));
    let mut next = timing(2, 3, -0.0);
    next.period_seconds = f64::from_bits(first.period_seconds.to_bits() + 1);
    assert_eq!(first.period_seconds as f32, next.period_seconds as f32);
    assert_ne!(first, next);
    assert!(adopt(&mut actual, &source, next));
    // Admission alone cannot relabel native/FIFO input that has not yet received a new feed.
    assert_eq!(history(&actual).binding.accepted, Some(first));
    assert_eq!(voice(&actual).stretch.pending_fifo_frames(), fifo);
    let output = render(&mut actual, 113);
    assert_eq!(output, render(&mut reference, 113));
    assert_eq!(voice(&actual).stretch.native_state_address(), native);
    let refreshed = history(&actual);
    assert_eq!(refreshed.binding.accepted, Some(next));
    assert_eq!(
        refreshed.binding.accepted.unwrap().origin_seconds.to_bits(),
        (-0.0_f64).to_bits()
    );
    assert_eq!(refreshed.fed_output_frames, before.fed_output_frames + 113);
    assert_eq!(
        actual.pad_dsp_chains[0].source_history().unwrap().binding,
        refreshed.binding
    );
    actual.clear_constant_timing(0, 3);
    assert_eq!(render(&mut actual, 211), render(&mut reference, 211));
    assert_eq!(history(&actual).binding.accepted, None);
    assert_eq!(voice(&actual).stretch.native_state_address(), native);
}

#[test]
fn productive_history_rate_ramps_pause_stem_transition_and_in_range_loop_keep_chronological_state()
{
    finite_productive_continuity_case();
    let source = sample(0.5);
    let mut mixer = Box::new(setup(&source));
    mixer.stop_sample(0);
    let stems = PreparedStemSet {
        complete_set_identity: std::sync::Arc::new([0; 32]),
        reference_samples: source.samples.clone(),
        reference_residency: source.residency.clone(),
        publication: PreparedSourcePermit::unrestricted(),
        source_version_hash: 42,
        sample_rate_hz: RATE,
        channels: 1,
        frame_count: source.samples.len(),
        available_mask: ((1_u16 << crate::messages::STEM_BUFFER_COUNT) - 1) as u8,
        accepted_timing: None,
        stems: std::array::from_fn(|_| sample(0.1)),
    };
    assert!(mixer.publish_prepared_stems(0, stems));
    assert!(mixer.play_sample(0, 1.0));
    render(&mut mixer, 1657);
    let before = history(&mixer);
    let native = voice(&mixer).stretch.native_state_address();
    mixer.pause_sample(0);
    assert!(render(&mut mixer, 127).iter().all(|value| *value == 0.0));
    assert_eq!(history(&mixer).fed_output_frames, before.fed_output_frames);
    assert_eq!(history(&mixer).next_position, before.next_position);
    mixer.resume_sample(0);
    mixer.set_speed(1.831_234_567_890_123);
    mixer.set_pad_loop_region(0, 0.01, Some(1.8)); // current position remains in range
    assert!(mixer.set_stem_mix_mode(0, StemMixMode::AllStems, 42));
    render(&mut mixer, 777);
    assert_eq!(voice(&mixer).stretch.native_state_address(), native);
    assert_eq!(
        history(&mixer).fed_output_frames,
        before.fed_output_frames + 777
    );
    assert!(voice(&mixer).source_playback.tempo_ratio() > 1.371_234_567_890_123);
    assert_eq!(
        history(&mixer).next_position,
        voice(&mixer).source_playback.position()
    );
}

#[test]
fn productive_history_clamp_discards_real_pending_fifos_and_never_reuses_dirty_native_on_failed_reserve()
 {
    finite_productive_failed_reserve_case();
    let source = sample(0.5);
    let mut mixer = Box::new(setup(&source));
    render(&mut mixer, 1657);
    let native = voice(&mixer).stretch.native_state_address();
    assert!(voice(&mixer).stretch.pending_fifo_frames().0 > 0);
    mixer.set_pad_loop_region(0, 1.0, Some(1.8)); // successful admitted physical source cut
    assert_eq!(mixer.loop_region_frames(0), (8000, Some(14_400)));
    assert!(voice(&mixer).stretch.productive_history().is_none());
    assert_eq!(voice(&mixer).stretch.pending_fifo_frames(), (0, 0));
    assert_ne!(voice(&mixer).stretch.native_state_address(), native);
    assert!(mixer.pad_dsp_chains[0].source_history().is_none());
    assert!(render(&mut mixer, 37).iter().all(|value| *value == 0.0));
    assert!(voice(&mixer).source_playback.position().frame >= RATE as usize);
    let mut wet_audible = false;
    for _ in 0..16 {
        wet_audible |= render(&mut mixer, 512)
            .iter()
            .any(|value| value.abs() > 0.01);
    }
    assert!(wet_audible);
    assert!(voice(&mixer).stretch.productive_history().is_some());

    let mut failed = Box::new(setup(&source));
    let mut reference = Box::new(setup(&source));
    assert_eq!(render(&mut failed, 1657), render(&mut reference, 1657));
    let before = history(&failed);
    let native = voice(&failed).stretch.native_state_address();
    let fifo = voice(&failed).stretch.pending_fifo_frames();
    let playback = voice(&failed).source_playback;
    let region = failed.loop_region_frames(0);
    let filter = failed.pad_dsp_chains[0].source_history().unwrap();
    voice(&failed).stretch.fail_preparation_worker();
    failed.set_pad_loop_region(0, 1.0, Some(1.8));
    assert_eq!(failed.loop_region_frames(0), region);
    assert!(!failed.seek_sample(0, playback.position().frame as f64 / f64::from(RATE)));
    assert_eq!(voice(&failed).stretch.native_state_address(), native);
    assert_eq!(voice(&failed).stretch.pending_fifo_frames(), fifo);
    assert!(voice(&failed).source_playback.matches_exact(&playback));
    assert_eq!(history(&failed).binding, before.binding);
    assert_eq!(history(&failed).fed_output_frames, before.fed_output_frames);
    assert_eq!(history(&failed).next_position, before.next_position);
    assert_eq!(
        failed.pad_dsp_chains[0]
            .source_history()
            .unwrap()
            .fed_output_frames,
        filter.fed_output_frames
    );
    let mut continued = 0;
    let mut audible = false;
    for frames in [37, 1, 127, 384, 512, 901] {
        let output = render(&mut failed, frames);
        assert_eq!(output, render(&mut reference, frames));
        audible |= output.iter().any(|sample| sample.abs() > 0.01);
        continued += frames as u64;
        assert_eq!(voice(&failed).stretch.native_state_address(), native);
        assert_eq!(
            history(&failed).fed_output_frames,
            before.fed_output_frames + continued
        );
    }
    assert!(audible);
}

fn finite_history_mixer(source: &SampleBuffer, start: usize, end: usize) -> Box<RtMixer> {
    let mut mixer = Box::new(RtMixer::new(1, RATE as f32));
    let ownership =
        Arc::new(crate::audio_engine::input_runtime_binding::InputRuntimeOwnership::tracked());
    ownership.publish_source(0, source, RATE, 1);
    mixer.set_input_runtime_ownership(ownership);
    mixer.load_sample(0, source.clone());
    mixer.set_pad_loop_region(
        0,
        start as f64 / f64::from(RATE),
        Some(end as f64 / f64::from(RATE)),
    );
    mixer.set_speed(1.371_234_567_890_123);
    mixer.set_pad_key_lock(0, true);
    assert!(mixer.play_sample_at_output_frame(0, 1.0, 0));
    mixer
}

fn finite_history_pair(start: usize, end: usize) -> (SampleBuffer, Box<RtMixer>, Box<RtMixer>) {
    let full = sample(0.5).with_complete_source(RATE);
    let finite = full
        .window(
            start,
            end,
            2,
            crate::messages::ResidentContext::KeyLockFiniteLoop,
        )
        .unwrap();
    (
        full.clone(),
        finite_history_mixer(&finite, start, end),
        finite_history_mixer(&full, start, end),
    )
}

fn finite_history_render(
    actual: &mut RtMixer,
    complete: &mut RtMixer,
    frame: &mut u64,
    frames: usize,
) -> Vec<f32> {
    crate::audio_engine::source_reader::reset_tap_observation_for_test();
    let mut output = vec![0.0; frames];
    actual.render_at_output_frame(*frame, &mut output, &mut [0.0; NUM_SAMPLES]);
    let reads = crate::audio_engine::source_reader::tap_observation_for_test();
    assert_eq!(reads.missing_reads, 0);
    if reads.left_reads > 0 {
        let source = voice(actual).sample.as_ref().unwrap();
        assert!(
            reads
                .min_frame
                .is_some_and(|frame| frame >= source.resident_start())
        );
        assert!(
            reads
                .max_frame
                .is_some_and(|frame| frame < source.resident_end())
        );
    }
    let mut expected = vec![0.0; frames];
    complete.render_at_output_frame(*frame, &mut expected, &mut [0.0; NUM_SAMPLES]);
    assert_eq!(output, expected, "finite productive-history at {frame}");
    assert!(
        voice(actual)
            .source_playback
            .matches_exact(&voice(complete).source_playback)
    );
    assert_eq!(
        voice(actual).stretch.pending_fifo_frames(),
        voice(complete).stretch.pending_fifo_frames()
    );
    *frame += frames as u64;
    output
}

fn finite_productive_seek_cases() {
    fn wait_for_cut_reserve(mixer: &RtMixer) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !voice(mixer).stretch.source_cut_available() {
            assert!(
                std::time::Instant::now() < deadline,
                "off-thread source-cut reserve did not become available"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    let (_, mut actual, mut complete) = finite_history_pair(0, 13_000);
    let mut frame = 0;
    for count in [512, 512, 512, 121] {
        finite_history_render(&mut actual, &mut complete, &mut frame, count);
    }
    assert!(voice(&actual).stretch.productive_history().is_some());
    let original_address = voice(&actual).sample.as_ref().unwrap().source_address();
    let replacement = SampleBuffer {
        residency: None,
        channels: 1,
        samples: vec![0.2; 2000].into(),
    }
    .with_complete_source(RATE);
    for mixer in [&mut actual, &mut complete] {
        mixer
            .input_runtime_ownership
            .publish_source(0, &replacement, RATE, 2);
        mixer.load_sample(0, replacement.clone());
        wait_for_cut_reserve(mixer);
        assert!(mixer.seek_sample_at_output_frame(0, 1.5, frame));
        assert_eq!(voice(mixer).source_playback.position().frame, 12_000);
        assert!(voice(mixer).stretch.productive_history().is_none());
        assert_eq!(voice(mixer).stretch.pending_fifo_frames(), (0, 0));
        assert!(mixer.pad_dsp_chains[0].source_history().is_none());
    }
    assert_eq!(
        voice(&actual).sample.as_ref().unwrap().source_address(),
        original_address
    );
    for count in [1, 127, 384, 96, 257, 512, 31, 901] {
        finite_history_render(&mut actual, &mut complete, &mut frame, count);
    }
    assert!(voice(&actual).stretch.productive_history().is_some());
    for mixer in [&mut actual, &mut complete] {
        // Audio frames do not give the recycle worker a wall-clock deadline.
        // This positive dirty-source cut requires its actual warmed reserve.
        wait_for_cut_reserve(mixer);
        assert!(mixer.seek_sample_at_output_frame(0, 1.5, frame));
        let generation = voice(mixer).generation;
        let position = voice(mixer).source_playback.position();
        // No wait here: the first cut left a clean unused handle that must
        // admit the immediate equal-phase seek and still cut its generation.
        assert!(mixer.seek_sample_at_output_frame(0, 1.5, frame));
        assert_eq!(voice(mixer).source_playback.position(), position);
        assert_ne!(voice(mixer).generation, generation);
        assert!(voice(mixer).stretch.productive_history().is_none());
        assert_eq!(voice(mixer).stretch.pending_fifo_frames(), (0, 0));
        assert!(mixer.pad_dsp_chains[0].source_history().is_none());
    }
    for count in [1, 127, 384, 96, 257, 512, 31, 901] {
        finite_history_render(&mut actual, &mut complete, &mut frame, count);
    }
}

fn finite_productive_continuity_case() {
    let (full, mut actual, mut complete) = finite_history_pair(80, 14_500);
    let finite = actual.sample_bank[0].as_ref().unwrap().clone();
    let stems = PreparedStemSet {
        complete_set_identity: Arc::new([71; 32]),
        reference_samples: full.samples.clone(),
        reference_residency: full.residency.clone(),
        publication: PreparedSourcePermit::unrestricted(),
        source_version_hash: 42,
        sample_rate_hz: RATE,
        channels: 1,
        frame_count: full.frame_count(),
        available_mask: ((1_u16 << crate::messages::STEM_BUFFER_COUNT) - 1) as u8,
        accepted_timing: None,
        stems: std::array::from_fn(|_| sample(0.1)),
    }
    .window_for(&full)
    .unwrap();
    // Components are independently generated complete PCM, then bound to the
    // source's actual full view by the same off-thread fixture preparation as
    // the finite view. Unbound component residency would correctly reject this set.
    assert!(
        crate::audio_engine::source_reader::prepared_stem_set_matches_sample(
            &stems,
            &full,
            1,
            RATE as f32,
            full.frame_count()
        )
    );
    for mixer in [&mut actual, &mut complete] {
        mixer.stop_sample(0);
    }
    assert!(actual.publish_prepared_stems(0, stems.clone().window_for(&finite).unwrap()));
    assert!(complete.publish_prepared_stems(0, stems));
    for mixer in [&mut actual, &mut complete] {
        assert!(mixer.play_sample_at_output_frame(0, 1.0, 0));
    }
    let mut frame = 0;
    finite_history_render(&mut actual, &mut complete, &mut frame, 1657);
    let native = voice(&actual).stretch.native_state_address();
    let before = history(&actual);
    for mixer in [&mut actual, &mut complete] {
        mixer.pause_sample_at_output_frame(0, frame);
    }
    assert!(
        finite_history_render(&mut actual, &mut complete, &mut frame, 127)
            .iter()
            .all(|sample| *sample == 0.0)
    );
    assert_eq!(history(&actual).fed_output_frames, before.fed_output_frames);
    for mixer in [&mut actual, &mut complete] {
        mixer.resume_sample_at_output_frame(0, frame);
        mixer.set_speed(1.831_234_567_890_123);
        mixer.set_pad_loop_region(0, 0.01, Some(1.8));
        assert_eq!(mixer.loop_region_frames(0), (80, Some(14_400)));
        assert!(mixer.set_stem_mix_mode(0, StemMixMode::AllStems, 42));
    }
    finite_history_render(&mut actual, &mut complete, &mut frame, 777);
    assert_eq!(voice(&actual).stretch.native_state_address(), native);
    assert_eq!(
        history(&actual).fed_output_frames,
        before.fed_output_frames + 777
    );
    assert!(voice(&actual).source_playback.tempo_ratio() > 1.371_234_567_890_123);
    assert_eq!(
        history(&actual).next_position,
        voice(&actual).source_playback.position()
    );
}

fn finite_productive_failed_reserve_case() {
    let (_, mut actual, mut complete) = finite_history_pair(0, 14_500);
    let mut frame = 0;
    finite_history_render(&mut actual, &mut complete, &mut frame, 1657);
    let native = voice(&actual).stretch.native_state_address();
    assert!(voice(&actual).stretch.pending_fifo_frames().0 > 0);
    let before = history(&actual);
    let fifo = voice(&actual).stretch.pending_fifo_frames();
    let playback = voice(&actual).source_playback;
    let filter = actual.pad_dsp_chains[0].source_history().unwrap();
    voice(&actual).stretch.fail_preparation_worker();
    actual.set_pad_loop_region(0, 1.0, Some(1.8));
    assert_eq!(actual.loop_region_frames(0), (0, Some(14_500)));
    assert!(!actual.seek_sample_at_output_frame(
        0,
        playback.position().frame as f64 / f64::from(RATE),
        frame
    ));
    assert_eq!(voice(&actual).stretch.native_state_address(), native);
    assert_eq!(voice(&actual).stretch.pending_fifo_frames(), fifo);
    assert!(voice(&actual).source_playback.matches_exact(&playback));
    assert_eq!(history(&actual).binding, before.binding);
    assert_eq!(history(&actual).fed_output_frames, before.fed_output_frames);
    assert_eq!(history(&actual).next_position, before.next_position);
    assert_eq!(
        actual.pad_dsp_chains[0]
            .source_history()
            .unwrap()
            .fed_output_frames,
        filter.fed_output_frames
    );
    let mut continued = 0;
    let mut audible = false;
    for frames in [37, 1, 127, 384, 512, 901] {
        let output = finite_history_render(&mut actual, &mut complete, &mut frame, frames);
        audible |= output.iter().any(|sample| sample.abs() > 0.01);
        continued += frames as u64;
        assert_eq!(voice(&actual).stretch.native_state_address(), native);
        assert_eq!(
            history(&actual).fed_output_frames,
            before.fed_output_frames + continued
        );
        assert!(voice(&actual).stretch.productive_history().is_some());
    }
    assert!(audible);
}

#[test]
fn productive_history_new_bank_timing_does_not_relabel_old_voice_and_retrigger_retires_old_pin() {
    let original = sample(0.5);
    let replacement = sample(0.2);
    let mut mixer = setup(&original);
    let first = timing(1, 2, 0.0);
    assert!(adopt(&mut mixer, &original, first));
    mixer.set_bpm_lock(true);
    mixer.set_master_period(first.period_seconds / 1.37);
    render(&mut mixer, 1657);
    let prior = history(&mixer);
    let native = voice(&mixer).stretch.native_state_address();
    let latest = timing(2, 3, -0.0);
    assert!(adopt(&mut mixer, &original, latest));
    // A bank replacement without any intervening render must retain the latest old-source timing.
    assert!(mixer.load_sample_rt(0, replacement.clone(), &mut ImmediateAudioBufferRetirement));
    let mut new_timing = timing(3, 4, 0.31);
    new_timing.period_seconds = 0.7;
    assert!(adopt(&mut mixer, &replacement, new_timing));
    mixer.set_pad_bpm(0, Some(70.0));
    render(&mut mixer, 37);
    assert_eq!(
        history(&mixer).binding.source_address,
        original.samples.as_ptr() as usize
    );
    assert_eq!(history(&mixer).binding.accepted, Some(latest));
    assert_eq!(
        history(&mixer).fed_output_frames,
        prior.fed_output_frames + 37
    );
    assert_eq!(voice(&mixer).source_playback.tempo_ratio(), 1.37);
    assert_eq!(voice(&mixer).stretch.native_state_address(), native);
    let current_position = voice(&mixer).source_playback.position();
    assert_eq!(
        mixer.active_pad_beat_position(0),
        voice(&mixer)
            .source_timing
            .grid(f64::from(RATE))
            .unwrap()
            .beat_at_source(current_position.frame as f64 + current_position.fraction)
    );
    let mut retirement = RetainedSamples {
        slots: 1,
        samples: Vec::new(),
    };
    assert!(mixer.play_sample_rt(0, 1.0, &mut retirement));
    assert_eq!(retirement.samples.len(), 1);
    assert!(Arc::ptr_eq(
        &retirement.samples[0].samples,
        &original.samples
    ));
    assert!(Arc::ptr_eq(
        &voice(&mixer).sample.as_ref().unwrap().samples,
        &replacement.samples
    ));
    assert!(voice(&mixer).stretch.productive_history().is_none());
    assert!(mixer.pad_dsp_chains[0].source_history().is_none());

    // The replacement's actual wet output must match a separately started native processor,
    // rather than merely carrying a new ledger tag over the old source's used native history.
    let mut independent = setup(&replacement);
    assert!(adopt(&mut independent, &replacement, new_timing));
    independent.set_pad_bpm(0, Some(70.0));
    independent.set_bpm_lock(true);
    independent.set_master_period(first.period_seconds / 1.37);
    assert!(independent.play_sample(0, 1.0));
    assert_eq!(render(&mut mixer, 37), render(&mut independent, 37));
    assert_ne!(voice(&mixer).stretch.native_state_address(), native);
    assert_eq!(history(&mixer).binding.accepted, Some(new_timing));
    assert_eq!(
        history(&mixer).binding.source_address,
        replacement.samples.as_ptr() as usize
    );
    let mut nonzero_native_output = false;
    for _ in 0..16 {
        for frames in [512, 1, 31, 257] {
            let actual = render(&mut mixer, frames);
            let expected = render(&mut independent, frames);
            assert_eq!(actual, expected);
            nonzero_native_output |= actual.iter().any(|sample| sample.abs() > 0.001);
            assert_eq!(
                voice(&mixer).source_playback.position(),
                voice(&independent).source_playback.position()
            );
        }
    }
    assert!(nonzero_native_output); // includes genuine shifted replacement content and source wrap
}

struct RetainedSamples {
    slots: usize,
    samples: Vec<SampleBuffer>,
}
impl AudioBufferRetirement for RetainedSamples {
    fn retire_resident_capture(
        &mut self,
        _: std::sync::Arc<crate::audio_engine::resident_seek::ResidentSeekCapture>,
    ) {
    }
    fn retire_resident_cancellation(&mut self, _: std::sync::Arc<std::sync::atomic::AtomicBool>) {}
    fn retire_resident_transaction(&mut self, _: Box<crate::messages::ResidentTransaction>) {}
    fn retire_cold_adoption(&mut self, _: Arc<std::sync::atomic::AtomicU8>) {}
    fn retire_accepted_timing_refresh(
        &mut self,
        _: Arc<crate::audio_engine::accepted_timing_refresh::AcceptedTimingRefresh>,
    ) {
    }
    fn retire_global_playback_batch(
        &mut self,
        _: Arc<crate::audio_engine::global_playback_batch::GlobalPlaybackBatch>,
    ) {
    }
    fn retire_sample(&mut self, sample: SampleBuffer) {
        assert!(self.slots > 0);
        self.slots -= 1;
        self.samples.push(sample);
    }
    fn retire_prepared_stems(&mut self, _: PreparedStemSet) {
        unreachable!()
    }
    fn retire_constant_timing(&mut self, _: PreparedConstantTiming) {
        unreachable!()
    }
    fn available_retirement_slots(&mut self) -> usize {
        self.slots
    }
}

#[test]
fn productive_history_failed_source_retrigger_admission_keeps_prior_pin_and_actual_state() {
    let source = sample(0.5);
    let mut mixer = setup(&source);
    render(&mut mixer, 1657);
    let before = history(&mixer);
    let fifo = voice(&mixer).stretch.pending_fifo_frames();
    let position = voice(&mixer).source_playback.position();
    mixer.load_sample(0, sample(0.2));
    assert!(!mixer.play_sample_rt(
        0,
        1.0,
        &mut RetainedSamples {
            slots: 0,
            samples: Vec::new()
        }
    ));
    assert_eq!(history(&mixer).binding, before.binding);
    assert_eq!(history(&mixer).fed_output_frames, before.fed_output_frames);
    assert_eq!(voice(&mixer).stretch.pending_fifo_frames(), fifo);
    assert_eq!(voice(&mixer).source_playback.position(), position);
    assert!(Arc::ptr_eq(
        &voice(&mixer).sample.as_ref().unwrap().samples,
        &source.samples
    ));
}

#[test]
fn productive_history_unavailable_automatic_rejects_new_admission_and_manual_tap_legacy_stay_unaccepted()
 {
    let source = sample(0.5);
    let mut mixer = setup(&source);
    render(&mut mixer, 1657);
    let before = history(&mixer);
    mixer
        .input_runtime_ownership
        .set_timing_intent(0, TimingIntent::Automatic);
    assert!(!mixer.play_sample(0, 1.0));
    assert_eq!(history(&mixer).fed_output_frames, before.fed_output_frames);
    render(&mut mixer, 37);
    assert_eq!(
        history(&mixer).fed_output_frames,
        before.fed_output_frames + 37
    );
    assert_eq!(history(&mixer).binding.accepted, None); // continuing Legacy source is never promoted
    for intent in [
        TimingIntent::Manual,
        TimingIntent::Tap,
        TimingIntent::Legacy,
    ] {
        mixer.input_runtime_ownership.set_timing_intent(0, intent);
        assert!(mixer.play_sample(0, 1.0));
        render(&mut mixer, 37);
        assert_eq!(history(&mixer).binding.accepted, None);
    }
    mixer.stop_sample(0);
    assert!(mixer.pad_dsp_chains[0].source_history().is_none());
    assert!(
        mixer
            .voices
            .iter()
            .all(|voice| voice.stretch.productive_history().is_none())
    );
}

#[test]
fn productive_history_paused_legacy_bank_replacement_freezes_own_period_origin_and_master_reference()
 {
    let source = sample(0.5);
    let mut mixer = setup(&source);
    render(&mut mixer, 77);
    mixer.pause_sample(0);
    mixer.set_pad_bpm(0, Some(123.456_789_012_345));
    mixer.set_pad_timing_metadata(
        0,
        PadTimingMetadata {
            phase_anchor_s: -0.017,
        },
    );
    mixer.load_sample(0, sample(0.2));
    mixer.set_pad_bpm(0, Some(71.0));
    mixer.set_pad_timing_metadata(
        0,
        PadTimingMetadata {
            phase_anchor_s: 0.8,
        },
    );
    mixer.resume_sample(0);
    assert_eq!(
        voice(&mixer).source_timing.legacy_period_seconds,
        Some(60.0 / 123.456_789_012_345)
    );
    assert_eq!(voice(&mixer).source_timing.legacy_origin_frame, -136.0);
    assert_eq!(
        mixer.transport_reference_period_for_sample_id(0),
        Some((60.0 / 123.456_789_012_345) / 1.371_234_567_890_123)
    );
    render(&mut mixer, 37);
    assert_eq!(
        history(&mixer).binding.source_address,
        source.samples.as_ptr() as usize
    );
    assert_eq!(history(&mixer).binding.accepted, None);
}
