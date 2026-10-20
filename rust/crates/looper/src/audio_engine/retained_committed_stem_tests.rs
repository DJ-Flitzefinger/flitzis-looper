//! Genuine committed residency on an already retained, offset Native input clock.
//! Device-free source chronology and PCM evidence; no device or hearing claim.

use super::*;
use crate::audio_engine::native_history_permit::NativeHistoryContext;
use crate::audio_engine::pitch_intent::PitchIntent;
use crate::audio_engine::prepared_native_history::{NativeAdapterState, RetainedNativeStartCut};
use crate::audio_engine::source_feed_history::SourceFeedSnapshot;
use crate::audio_engine::source_playback::SourcePlayback;
use crate::audio_engine::stretch_processor::ProductiveSourceFeed;

const RETAINED_REQUEST: u64 = 0x5245_5441_494e;

fn install_retained_native(p: &mut Productive) -> (SourcePlayback, SourcePlayback, usize) {
    let lane =
        p.h.callback
            .mixer
            .voices
            .iter()
            .position(|voice| voice.is_playing_sample(0))
            .unwrap();
    let snapshot =
        p.h.callback
            .mixer
            .capture_pitch_voice_snapshot(lane)
            .unwrap();
    let logical = snapshot.playback;
    let plan = snapshot.plan;
    let sample = snapshot.sample.clone();
    let accepted = snapshot.accepted;
    // This is the engine's stable source owner, distinct from the Native worker lane epoch.
    let source_epoch = p.h.engine.prepared_source_epoch(0).unwrap().clone();
    let epoch = p.h.callback.mixer.voices[lane].stretch.preparation_epoch();
    let permit = NativeHistoryContext {
        id: 0,
        ownership: &p.h.engine.input_runtime_ownership,
        acknowledgements: &p.h.engine.current_timing_acknowledgements,
        preparation_epoch: &source_epoch,
    }
    .capture(&sample, RATE, accepted)
    .unwrap();
    let request = snapshot
        .into_request(
            permit.clone(),
            PitchIntent::default(),
            true,
            p.frame,
            RETAINED_REQUEST,
            epoch,
        )
        .unwrap();
    assert!(request.coverage.is_some(), "genuine finite source coverage");
    let mut candidate = Box::new(NativeAdapterState::new(
        Some(RubberBandLiveShifter::new(RATE, 2).unwrap()),
        2,
    ));
    candidate
        .prepare_retained_source_start_checked(
            request,
            Some(RetainedNativeStartCut {
                history_output_frames: 4096,
                discarded_raw_output_frames: 4233,
            }),
        )
        .unwrap();
    let (actual, _) = candidate
        .retained_start_feed(&logical, plan)
        .unwrap()
        .unwrap();
    let feed = ProductiveSourceFeed {
        pitch_intent: PitchIntent::default(),
        key_lock: true,
        sample: &sample,
        stems: None,
        outgoing_stems: None,
        sample_rate_hz: RATE,
        accepted,
        plan,
        playback: &logical,
        permit: Some(&permit),
        output_frame: Some(p.frame),
    };
    assert!(
        p.h.callback.mixer.voices[lane]
            .stretch
            .adopt_prepared_pitch(&mut candidate, &feed, epoch, RETAINED_REQUEST,)
    );
    let native = voice(&p.h.callback.mixer).stretch.native_state_address();
    assert_ne!(native, 0);
    assert_ne!(actual.position(), logical.position());
    (logical, actual, native)
}

fn render_actual(p: &mut Productive, frames: usize) -> Vec<f32> {
    let mut output = vec![0.0; frames * 2];
    let callback = p.h.callback.as_mut();
    reset_tap_observation_for_test();
    callback.mixer.render_rt_at_output_frame(
        &mut output,
        &mut [0.0; NUM_SAMPLES],
        p.frame,
        &mut callback.retirement,
    );
    assert_eq!(tap_observation_for_test().missing_reads, 0);
    assert!(output.iter().all(|sample| sample.is_finite()));
    p.frame += frames as u64;
    output
}

fn assert_actual_continuity(
    p: &Productive,
    logical: &SourcePlayback,
    actual: &SourcePlayback,
    elapsed: usize,
) {
    let current = voice(&p.h.callback.mixer);
    assert_eq!(
        current.source_playback.position(),
        logical.position_at(elapsed)
    );
    let history = current.stretch.productive_history().unwrap();
    assert_eq!(
        history.next_position,
        actual.position_at(elapsed),
        "committed stem control lost the retained actual input clock"
    );
    assert_ne!(history.next_position, current.source_playback.position());
    let sample = current.sample.as_ref().unwrap();
    assert_eq!(history.binding.source_address, sample.source_address());
    assert_eq!(history.binding.resident, sample.resident_binding());
    let mut journal = SourceFeedSnapshot::new();
    assert!(current.source_feed_history.capture_into(&mut journal));
    let last = journal.spans().last().unwrap();
    assert_eq!(
        last.playback.position_at(last.frames),
        history.next_position
    );
    assert_eq!(last.context.binding.source_address, sample.source_address());
}

fn render_paused_actual(
    p: &mut Productive,
    logical: &SourcePlayback,
    actual: &SourcePlayback,
    elapsed: usize,
    frames: usize,
) {
    let current = voice(&p.h.callback.mixer);
    assert!(current.paused);
    let phase = current.source_playback;
    let native = current.stretch.native_state_address();
    let origin = current.stretch.native_origin_tuple();
    let fifo = current.stretch.pending_fifo_frames();
    let history = current.stretch.productive_history().unwrap();
    let transition = p.h.callback.mixer.stem_transition_for_measurement(0);
    let output = render_actual(p, frames);
    assert!(output.iter().all(|sample| *sample == 0.0));
    assert!(tap_observation_for_test().min_frame.is_none());
    assert_actual_continuity(p, logical, actual, elapsed);
    let current = voice(&p.h.callback.mixer);
    assert!(current.paused && current.source_playback.matches_exact(&phase));
    assert_eq!(current.stretch.native_state_address(), native);
    assert_eq!(current.stretch.native_origin_tuple(), origin);
    assert_eq!(current.stretch.pending_fifo_frames(), fifo);
    let after = current.stretch.productive_history().unwrap();
    assert_eq!(after.next_position, history.next_position);
    assert_eq!(after.fed_output_frames, history.fed_output_frames);
    assert!(
        p.h.callback
            .mixer
            .stem_transition_for_measurement(0)
            .matches_exact(transition)
    );
}

fn committed_round_trip(paused: bool) {
    let mut p = Productive::new(true, 1.37);
    let (logical, actual, native) = install_retained_native(&mut p);
    let generation = voice(&p.h.callback.mixer).generation;
    let mut elapsed = 0;
    let mut before = Vec::new();
    for frames in [17, 31, 1, 511] {
        before.extend(render_actual(&mut p, frames));
        elapsed += frames;
        assert_actual_continuity(&p, &logical, &actual, elapsed);
    }
    assert!(before.iter().any(|sample| sample.abs() > 0.001));

    // These are the genuine selected disk pair, control capture, worker and callback Adopt.
    if paused {
        p.h.callback.mixer.pause_sample_at_output_frame(0, p.frame);
    }
    let ticket = p.request("all_stems", 0b1010);
    p.capture_then_wait(&ticket);
    assert_eq!(
        p.h.callback.mixer.stem_demand_for_measurement(0).0,
        StemMixMode::FullMix
    );
    if paused {
        render_paused_actual(&mut p, &logical, &actual, elapsed, 513);
    } else {
        render_actual(&mut p, 31);
        elapsed += 31;
    }
    assert_actual_continuity(&p, &logical, &actual, elapsed);
    let current = voice(&p.h.callback.mixer);
    let phase = current.source_playback;
    let origin = current.stretch.native_origin_tuple();
    let fifo = current.stretch.pending_fifo_frames();
    let history = current.stretch.productive_history().unwrap();
    assert_eq!(current.stretch.native_state_address(), native);
    assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
    assert_eq!(
        ticket.residency_status(),
        "accepted",
        "{:?}",
        ticket.error()
    );
    assert_eq!(ticket.effective_mode_status(), "transitioning");
    assert!(ticket.is_current() && p.selected.is_current());
    let current = voice(&p.h.callback.mixer);
    assert_eq!(current.generation, generation);
    assert!(current.source_playback.matches_exact(&phase));
    assert_eq!(current.stretch.native_state_address(), native);
    assert_eq!(current.stretch.native_origin_tuple(), origin);
    assert_eq!(current.stretch.pending_fifo_frames(), fifo);
    let after = current.stretch.productive_history().unwrap();
    assert_eq!(after.binding, history.binding);
    assert_eq!(after.next_position, history.next_position);
    assert_eq!(after.fed_output_frames, history.fed_output_frames);
    if paused {
        for frames in [1, 127, 384, 512] {
            render_paused_actual(&mut p, &logical, &actual, elapsed, frames);
            assert_eq!(ticket.effective_mode_status(), "transitioning");
            assert!(p.h.callback.mixer.stems_for_measurement()[0].is_some());
        }
        // The established device-free fixture's ordinary Resume changes transport state only.
        p.h.callback.mixer.resume_sample_at_output_frame(0, p.frame);
    }

    let mut continued = Vec::new();
    for frames in [1, 17, 31, 511, 257, 1, 512, 73] {
        continued.extend(render_actual(&mut p, frames));
        elapsed += frames;
        assert_actual_continuity(&p, &logical, &actual, elapsed);
        assert_eq!(voice(&p.h.callback.mixer).generation, generation);
        assert_eq!(
            voice(&p.h.callback.mixer).stretch.native_state_address(),
            native
        );
    }
    assert!(continued.iter().any(|sample| sample.abs() > 0.001));
    assert!(
        !p.h.callback
            .mixer
            .stem_transition_for_measurement(0)
            .is_active()
    );

    // The accepted physical read context is copied into the existing Native owner.
    assert_eq!(
        voice(&p.h.callback.mixer).stretch.native_state_address(),
        native
    );
    assert_eq!(
        ticket.effective_mode_status(),
        "effective",
        "{:?}",
        ticket.error()
    );
    assert!(ticket.error().is_none());
    let mut effective_pcm = Vec::new();
    for frames in [511, 257, 1, 512, 137] {
        effective_pcm.extend(render_actual(&mut p, frames));
        elapsed += frames;
        assert_actual_continuity(&p, &logical, &actual, elapsed);
        assert_eq!(voice(&p.h.callback.mixer).generation, generation);
    }
    assert!(effective_pcm.iter().any(|sample| sample.abs() > 0.001));
    assert_eq!(ticket.effective_mode_status(), "effective");
    let resident = p.h.callback.mixer.stems_for_measurement()[0]
        .as_ref()
        .unwrap();
    let stem_owner = Arc::as_ptr(&resident.complete_set_identity) as usize;
    let mut journal = SourceFeedSnapshot::new();
    assert!(
        voice(&p.h.callback.mixer)
            .source_feed_history
            .capture_into(&mut journal)
    );
    let last = journal.spans().last().unwrap();
    assert_eq!(last.context.current_stem_owners[0], stem_owner);
    assert_eq!(p.h.callback.mixer.stem_demand_for_measurement(0).1, 0b1010);
    assert_eq!(
        voice(&p.h.callback.mixer).stretch.native_state_address(),
        native
    );

    // A weak observation creates no additional reader pin. Native history may retain
    // the immutable original request after the live mixer slots have been released.
    let readers = resident
        .stems
        .each_ref()
        .map(|stem| Arc::downgrade(&stem.samples));
    if paused {
        p.h.callback.mixer.pause_sample_at_output_frame(0, p.frame);
    }
    let release = p.request("full_mix", 0b1010);
    p.capture_then_wait(&release);
    let current = voice(&p.h.callback.mixer);
    let phase = current.source_playback;
    let origin = current.stretch.native_origin_tuple();
    let fifo = current.stretch.pending_fifo_frames();
    let history = current.stretch.productive_history().unwrap();
    assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
    assert_eq!(
        release.residency_status(),
        "accepted",
        "{:?}",
        release.error()
    );
    assert_eq!(release.effective_mode_status(), "transitioning");
    let current = voice(&p.h.callback.mixer);
    assert_eq!(current.generation, generation);
    assert!(current.source_playback.matches_exact(&phase));
    assert_eq!(current.stretch.native_state_address(), native);
    assert_eq!(current.stretch.native_origin_tuple(), origin);
    assert_eq!(current.stretch.pending_fifo_frames(), fifo);
    let after = current.stretch.productive_history().unwrap();
    assert_eq!(after.binding, history.binding);
    assert_eq!(after.next_position, history.next_position);
    assert_eq!(after.fed_output_frames, history.fed_output_frames);
    assert!(readers.iter().all(|reader| reader.strong_count() > 0));
    assert!(p.h.callback.mixer.stems_for_measurement()[0].is_some());

    if paused {
        for frames in [1, 127, 384, 512] {
            render_paused_actual(&mut p, &logical, &actual, elapsed, frames);
            assert_eq!(release.effective_mode_status(), "transitioning");
            assert!(p.h.callback.mixer.stems_for_measurement()[0].is_some());
            assert!(readers.iter().all(|reader| reader.strong_count() > 0));
        }
        p.h.callback.mixer.resume_sample_at_output_frame(0, p.frame);
    }
    let mut released_pcm = render_actual(&mut p, 31);
    elapsed += 31;
    assert_actual_continuity(&p, &logical, &actual, elapsed);
    assert!(
        p.h.callback
            .mixer
            .stem_transition_for_measurement(0)
            .is_active()
    );
    assert!(p.h.callback.mixer.stems_for_measurement()[0].is_some());
    assert!(readers.iter().all(|reader| reader.strong_count() > 0));
    if paused {
        // Pausing a partly consumed release must retain its actual reader and ramp,
        // even when wall-clock callback frames exceed the whole 128-source-frame ramp.
        p.h.callback.mixer.pause_sample_at_output_frame(0, p.frame);
        for frames in [1, 127, 384, 512] {
            render_paused_actual(&mut p, &logical, &actual, elapsed, frames);
            assert_eq!(release.effective_mode_status(), "transitioning");
            assert!(p.h.callback.mixer.stems_for_measurement()[0].is_some());
            assert!(readers.iter().all(|reader| reader.strong_count() > 0));
        }
        p.h.callback.mixer.resume_sample_at_output_frame(0, p.frame);
    }
    let mut remaining = (128.0_f64 / 1.37).ceil() as usize - 31;
    for partition in [1_usize, 17, 7, 31].into_iter().cycle() {
        let frames = partition.min(remaining);
        released_pcm.extend(render_actual(&mut p, frames));
        elapsed += frames;
        remaining -= frames;
        assert_actual_continuity(&p, &logical, &actual, elapsed);
        assert_eq!(
            voice(&p.h.callback.mixer).stretch.native_state_address(),
            native
        );
        if remaining == 0 {
            break;
        }
        assert!(
            p.h.callback
                .mixer
                .stem_transition_for_measurement(0)
                .is_active()
        );
        assert!(p.h.callback.mixer.stems_for_measurement()[0].is_some());
        assert!(readers.iter().all(|reader| reader.strong_count() > 0));
    }
    assert!(
        !p.h.callback
            .mixer
            .stem_transition_for_measurement(0)
            .is_active()
    );
    for frames in [1, 17, 31, 511, 257, 512] {
        released_pcm.extend(render_actual(&mut p, frames));
        elapsed += frames;
        assert_actual_continuity(&p, &logical, &actual, elapsed);
        assert_eq!(voice(&p.h.callback.mixer).generation, generation);
        assert_eq!(
            voice(&p.h.callback.mixer).stretch.native_state_address(),
            native
        );
    }
    assert!(released_pcm.iter().any(|sample| sample.abs() > 0.001));
    assert_eq!(
        release.effective_mode_status(),
        "effective",
        "{:?}",
        release.error()
    );
    assert!(release.error().is_none() && p.selected.is_current());
    assert_eq!(
        p.h.callback.mixer.stem_demand_for_measurement(0).0,
        StemMixMode::FullMix
    );
    assert!(p.h.callback.mixer.stems_for_measurement()[0].is_none());
    assert!(
        p.h.callback
            .mixer
            .outgoing_stems_for_measurement(0)
            .is_none()
    );
    let mut journal = SourceFeedSnapshot::new();
    assert!(
        voice(&p.h.callback.mixer)
            .source_feed_history
            .capture_into(&mut journal)
    );
    let last = journal.spans().last().unwrap();
    assert!(
        last.context
            .current_stem_owners
            .iter()
            .all(|owner| *owner == 0)
    );
    assert!(
        last.context
            .outgoing_stem_owners
            .iter()
            .all(|owner| *owner == 0)
    );
}

#[test]
fn genuine_committed_capture_adopt_keeps_retained_native_and_actual_source_clock() {
    committed_round_trip(false);
}

#[test]
fn paused_committed_round_trip_holds_retained_reader_until_actual_release_ramp_ends() {
    committed_round_trip(true);
}

fn stem_reader_markers(stems: &PreparedStemSet) -> Vec<usize> {
    let mut markers = vec![
        Arc::as_ptr(&stems.complete_set_identity) as usize,
        stems.reference_samples.as_ptr() as usize,
    ];
    markers.extend(
        stems
            .stems
            .iter()
            .map(|stem| stem.samples.as_ptr() as usize),
    );
    markers
}

fn assert_journal_reader_roles(p: &Productive, current: &[usize], outgoing: &[usize]) {
    let mut journal = SourceFeedSnapshot::new();
    assert!(
        voice(&p.h.callback.mixer)
            .source_feed_history
            .capture_into(&mut journal)
    );
    let last = journal.spans().last().unwrap();
    assert_eq!(last.context.current_stem_owners.as_slice(), current);
    assert_eq!(last.context.outgoing_stem_owners.as_slice(), outgoing);
}

#[test]
fn genuine_retained_candidate_preserves_current_outgoing_reader_roles_through_active_and_paused_ramp()
 {
    for paused in [false, true] {
        let mut p = Productive::new(true, 1.37);
        let (logical, actual, native) = install_retained_native(&mut p);
        let generation = voice(&p.h.callback.mixer).generation;
        let mut elapsed = 0;
        for frames in [17, 31, 1, 511] {
            render_actual(&mut p, frames);
            elapsed += frames;
            assert_actual_continuity(&p, &logical, &actual, elapsed);
        }
        let first = p.request("all_stems", 0b1010);
        p.capture_then_wait(&first);
        assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
        for frames in [1, 17, 31, 511, 257, 1, 512, 119] {
            render_actual(&mut p, frames);
            elapsed += frames;
            assert_actual_continuity(&p, &logical, &actual, elapsed);
            assert_eq!(
                voice(&p.h.callback.mixer).stretch.native_state_address(),
                native
            );
        }
        assert_eq!(
            first.effective_mode_status(),
            "effective",
            "{:?}",
            first.error()
        );
        let old = p.h.callback.mixer.stems_for_measurement()[0]
            .as_ref()
            .unwrap();
        let old_markers = stem_reader_markers(old);
        let old_readers = old
            .stems
            .each_ref()
            .map(|stem| Arc::downgrade(&stem.samples));
        assert_journal_reader_roles(&p, &old_markers, &vec![0; old_markers.len()]);

        let (_pair, registration) =
            super::l3_native_tests::candidate(&mut p, 211 + usize::from(paused));
        if paused {
            p.h.callback.mixer.pause_sample_at_output_frame(0, p.frame);
        }
        let receipt = super::l3_native_tests::request_candidate(&mut p, &registration, 0b1010);
        p.capture_then_wait(&receipt);
        if paused {
            render_paused_actual(&mut p, &logical, &actual, elapsed, 513);
        } else {
            render_actual(&mut p, 31);
            elapsed += 31;
            assert_actual_continuity(&p, &logical, &actual, elapsed);
        }
        let current = voice(&p.h.callback.mixer);
        let phase = current.source_playback;
        let origin = current.stretch.native_origin_tuple();
        let fifo = current.stretch.pending_fifo_frames();
        let history = current.stretch.productive_history().unwrap();
        assert_eq!(
            p.h.callback.drain(&mut p.h.consumer),
            1,
            "own V2 candidate Adopt"
        );
        assert_eq!(
            receipt.residency_status(),
            "accepted",
            "{:?}",
            receipt.error()
        );
        assert_eq!(receipt.effective_mode_status(), "transitioning");
        assert!(receipt.claimed() && registration.is_current());
        let current = voice(&p.h.callback.mixer);
        assert_eq!(current.generation, generation);
        assert!(current.source_playback.matches_exact(&phase));
        assert_eq!(current.stretch.native_state_address(), native);
        assert_eq!(current.stretch.native_origin_tuple(), origin);
        assert_eq!(current.stretch.pending_fifo_frames(), fifo);
        let after = current.stretch.productive_history().unwrap();
        assert_eq!(after.binding, history.binding);
        assert_eq!(after.next_position, history.next_position);
        assert_eq!(after.fed_output_frames, history.fed_output_frames);
        let new_markers = stem_reader_markers(
            p.h.callback.mixer.stems_for_measurement()[0]
                .as_ref()
                .unwrap(),
        );
        assert_ne!(new_markers[0], old_markers[0]);
        let outgoing =
            p.h.callback
                .mixer
                .outgoing_stems_for_measurement(0)
                .unwrap();
        assert_eq!(stem_reader_markers(outgoing), old_markers);
        assert!(old_readers.iter().all(|reader| reader.strong_count() > 0));
        if paused {
            for frames in [1, 127, 384, 512] {
                render_paused_actual(&mut p, &logical, &actual, elapsed, frames);
                assert_eq!(receipt.effective_mode_status(), "transitioning");
                assert_eq!(
                    stem_reader_markers(
                        p.h.callback
                            .mixer
                            .outgoing_stems_for_measurement(0)
                            .unwrap()
                    ),
                    old_markers
                );
                assert!(old_readers.iter().all(|reader| reader.strong_count() > 0));
            }
            p.h.callback.mixer.resume_sample_at_output_frame(0, p.frame);
        }

        // The first genuine input receipt must name V2 as current and V1 as outgoing.
        let mut output = render_actual(&mut p, 1);
        elapsed += 1;
        assert_actual_continuity(&p, &logical, &actual, elapsed);
        assert_journal_reader_roles(&p, &new_markers, &old_markers);
        output.extend(render_actual(&mut p, 31));
        elapsed += 31;
        assert_actual_continuity(&p, &logical, &actual, elapsed);
        assert_journal_reader_roles(&p, &new_markers, &old_markers);
        if paused {
            p.h.callback.mixer.pause_sample_at_output_frame(0, p.frame);
            for frames in [1, 127, 384, 512] {
                render_paused_actual(&mut p, &logical, &actual, elapsed, frames);
                assert_eq!(receipt.effective_mode_status(), "transitioning");
                assert_journal_reader_roles(&p, &new_markers, &old_markers);
                assert_eq!(
                    stem_reader_markers(
                        p.h.callback
                            .mixer
                            .outgoing_stems_for_measurement(0)
                            .unwrap()
                    ),
                    old_markers
                );
            }
            p.h.callback.mixer.resume_sample_at_output_frame(0, p.frame);
        }
        let mut remaining = (128.0_f64 / 1.37).ceil() as usize - 32;
        for partition in [1_usize, 17, 7, 31].into_iter().cycle() {
            let frames = partition.min(remaining);
            output.extend(render_actual(&mut p, frames));
            elapsed += frames;
            remaining -= frames;
            assert_actual_continuity(&p, &logical, &actual, elapsed);
            assert_journal_reader_roles(&p, &new_markers, &old_markers);
            assert_eq!(
                voice(&p.h.callback.mixer).stretch.native_state_address(),
                native
            );
            if remaining == 0 {
                break;
            }
            assert!(
                p.h.callback
                    .mixer
                    .stem_transition_for_measurement(0)
                    .is_active()
            );
            assert_eq!(
                stem_reader_markers(
                    p.h.callback
                        .mixer
                        .outgoing_stems_for_measurement(0)
                        .unwrap()
                ),
                old_markers
            );
            assert!(old_readers.iter().all(|reader| reader.strong_count() > 0));
        }
        assert!(
            !p.h.callback
                .mixer
                .stem_transition_for_measurement(0)
                .is_active()
        );
        for frames in [1, 17, 31, 511, 257, 512] {
            output.extend(render_actual(&mut p, frames));
            elapsed += frames;
            assert_actual_continuity(&p, &logical, &actual, elapsed);
            assert_journal_reader_roles(&p, &new_markers, &vec![0; new_markers.len()]);
            assert_eq!(voice(&p.h.callback.mixer).generation, generation);
            assert_eq!(
                voice(&p.h.callback.mixer).stretch.native_state_address(),
                native
            );
            assert!(
                p.h.callback
                    .mixer
                    .outgoing_stems_for_measurement(0)
                    .is_none()
            );
        }
        assert!(output.iter().any(|sample| sample.abs() > 0.001));
        assert_eq!(
            receipt.effective_mode_status(),
            "effective",
            "{:?}",
            receipt.error()
        );
        assert!(receipt.error().is_none() && registration.is_current());
        assert_eq!(
            stem_reader_markers(
                p.h.callback.mixer.stems_for_measurement()[0]
                    .as_ref()
                    .unwrap()
            ),
            new_markers
        );
    }
}
