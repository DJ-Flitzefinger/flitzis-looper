//! Actual callback publication and rendering of complete authority/finite storage.
use super::*;
use crate::audio_engine::audio_stream::process_control_message;
use crate::audio_engine::input_runtime_binding::{InputPadBinding, InputRuntimeOwnership};
use crate::audio_engine::prepared_source::PreparedSourcePermit;
use crate::audio_engine::scheduler::FixedCapacityScheduler;
use crate::audio_engine::transport::TransportTimeline;
use crate::messages::{ControlMessage, ResidentContext, TriggerQuantization};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

const RATE: u32 = 48_000;
const START: usize = 512;
const END: usize = 2012;

fn complete() -> SampleBuffer {
    SampleBuffer {
        residency: None,
        channels: 1,
        samples: Arc::from(
            (0..9000)
                .map(|n| (n as f32 * 0.073).sin() * 0.4)
                .collect::<Vec<_>>(),
        ),
    }
    .with_complete_source(RATE)
}

fn accepted() -> AcceptedTimingProjection {
    AcceptedTimingProjection {
        revision: [21; 32],
        period_seconds: 24004.0 / f64::from(RATE),
        origin_seconds: START as f64 / f64::from(RATE),
        sample_rate_hz: RATE,
        publication_epoch: 4,
    }
}

fn fixture(sample: &SampleBuffer, stems: Option<PreparedStemSet>, wet: bool) -> RtMixer {
    let mut mixer = RtMixer::new(1, RATE as f32);
    let ownership = Arc::new(InputRuntimeOwnership::tracked());
    ownership.publish_source(0, sample, RATE, 1);
    mixer.set_input_runtime_ownership(ownership);
    mixer.load_sample(0, sample.clone());
    let permit = PreparedSourcePermit::unrestricted();
    permit.mark_pending().unwrap();
    assert!(mixer.publish_constant_timing_rt(
        0,
        PreparedConstantTiming {
            reference: sample.clone(),
            publication: permit,
            projection: accepted(),
        },
        &mut ImmediateAudioBufferRetirement
    ));
    if let Some(mut stems) = stems {
        stems.accepted_timing = Some(accepted());
        assert!(mixer.publish_prepared_stems(0, stems));
        mixer.set_stem_mix_mode(0, StemMixMode::AllStems, 91);
        mixer.set_stem_enabled_mask(0, 0b1010, 91);
    }
    mixer.set_pad_loop_region(
        0,
        START as f64 / f64::from(RATE),
        Some(END as f64 / f64::from(RATE)),
    );
    mixer.set_speed(0.73);
    mixer.set_pad_eq(0, 3.0, -2.0, 1.0);
    mixer.set_pad_key_lock(0, wet);
    assert!(mixer.play_sample_at_output_frame(0, 1.0, 0));
    mixer
}

fn voice(mixer: &RtMixer) -> &VoiceSlot {
    mixer.voices.iter().find(|voice| voice.active).unwrap()
}
fn voice_mut(mixer: &mut RtMixer) -> &mut VoiceSlot {
    mixer.voices.iter_mut().find(|voice| voice.active).unwrap()
}

fn render(mixer: &mut RtMixer, start: u64, frames: usize) -> Vec<f32> {
    let mut out = vec![0.0; frames];
    let mut peaks = [0.0; NUM_SAMPLES];
    mixer.render_at_output_frame(start, &mut out, &mut peaks);
    out
}

fn binding(mixer: &RtMixer) -> InputPadBinding {
    let sample = mixer.sample_bank[0].as_ref().unwrap();
    InputPadBinding {
        resident: sample.resident_binding(),
        source_address: sample.source_address(),
        sample_count: sample.source_sample_count(),
        channels: 1,
        sample_rate_hz: RATE,
        authority_revision: mixer.input_runtime_ownership.authority[0].load(Ordering::Acquire),
        runtime_revision: 0,
        accepted: mixer.pad_accepted_timing[0],
    }
}

fn apply(mixer: &mut RtMixer, command: ControlMessage) {
    process_control_message(
        command,
        &mut FixedCapacityScheduler::<8>::new(),
        0,
        &mut TriggerQuantization::Immediate,
        &mut TransportTimeline::new(RATE),
        mixer,
        &mut Vec::new(),
        &mut ImmediateAudioBufferRetirement,
    );
}

fn stems(sample: &SampleBuffer) -> PreparedStemSet {
    PreparedStemSet {
        complete_set_identity: Arc::new([22; 32]),
        accepted_timing: None,
        reference_samples: sample.samples.clone(),
        publication: PreparedSourcePermit::unrestricted(),
        source_version_hash: 91,
        sample_rate_hz: RATE,
        channels: 1,
        frame_count: sample.frame_count(),
        available_mask: ((1_u16 << crate::messages::STEM_BUFFER_COUNT) - 1) as u8,
        stems: [0.1, 0.2, 0.3, 0.4].map(|gain| SampleBuffer {
            residency: sample.residency.clone(),
            channels: 1,
            samples: Arc::from(
                sample
                    .samples
                    .iter()
                    .map(|value| value * gain)
                    .collect::<Vec<_>>(),
            ),
        }),
    }
}

#[test]
fn storage_only_callback_relocation_retains_source_current_voice_rate_filter_and_stem_transition() {
    for with_stems in [false, true] {
        let full = complete();
        let finite = full
            .window(START, END, 1, ResidentContext::FiniteLoop)
            .unwrap();
        let finite_stems = with_stems.then(|| stems(&full).window_for(&finite).unwrap());
        let mut actual = fixture(&finite, finite_stems.clone(), false);
        let mut reference = fixture(&finite, finite_stems.clone(), false);
        assert_eq!(render(&mut actual, 0, 137), render(&mut reference, 0, 137));
        for mixer in [&mut actual, &mut reference] {
            mixer.set_speed(1.25);
            if with_stems {
                mixer.set_stem_enabled_mask(0, 0b0101, 91);
            }
        }
        assert_eq!(
            render(&mut actual, 137, 31),
            render(&mut reference, 137, 31)
        );
        let before = voice(&actual).source_playback;
        let timing = actual.pad_accepted_timing[0];
        let old_binding = binding(&actual);
        let next = full
            .window(START - 19, END + 29, 2, ResidentContext::FiniteLoop)
            .unwrap();
        let next_stems = finite_stems.map(|accepted_set| {
            let mut new = stems(&full);
            new.complete_set_identity = accepted_set.complete_set_identity;
            new.accepted_timing = timing;
            new.window_for(&next).unwrap()
        });
        let publication = PreparedSourcePermit::new(
            actual.prepared_source_epochs[0].clone(),
            actual.prepared_source_epochs[0].load(Ordering::Acquire),
        );
        publication.mark_pending().unwrap();
        apply(
            &mut actual,
            ControlMessage::RelocateResident(Box::new(crate::messages::ResidentTransaction {
                id: 0,
                sample: next.clone(),
                stems: next_stems,
                binding: old_binding,
                publication: publication.clone(),
                expected_window_revision: 1,
                intent: Default::default(),
                seek_pin: None,
            })),
        );
        assert_eq!(publication.status(), "accepted");
        assert!(voice(&actual).source_playback.matches_exact(&before));
        assert_eq!(actual.pad_accepted_timing[0], timing);
        assert!(
            !actual
                .input_runtime_ownership
                .binding_source_current(0, old_binding)
        );
        assert!(actual.source_binding_current(0, binding(&actual)));
        assert_eq!(
            voice(&actual).sample.as_ref().unwrap().resident_binding(),
            next.resident_binding()
        );
        let mut elapsed = 168;
        for frames in [1, 127, 384, 96, 257, 512, 31, 901] {
            assert_eq!(
                render(&mut actual, elapsed, frames),
                render(&mut reference, elapsed, frames)
            );
            elapsed += frames as u64;
        }
        assert!(
            voice(&actual)
                .source_playback
                .matches_exact(&voice(&reference).source_playback)
        );
    }
}

#[test]
fn active_relocation_rejects_new_source_new_complete_stemset_stale_revision_cancelled_and_unavailable_context()
 {
    for case in 0..6 {
        let full = complete();
        let finite = full
            .window(START, END, 1, ResidentContext::FiniteLoop)
            .unwrap();
        let old_stems = stems(&full).window_for(&finite).unwrap();
        let mut actual = fixture(&finite, Some(old_stems.clone()), false);
        let mut reference = fixture(&finite, Some(old_stems.clone()), false);
        assert_eq!(render(&mut actual, 0, 77), render(&mut reference, 0, 77));
        let before = binding(&actual);
        let mut next = full
            .window(START - 2, END + 2, 2, ResidentContext::FiniteLoop)
            .unwrap();
        if case == 0 {
            next = complete()
                .window(START - 2, END + 2, 2, ResidentContext::FiniteLoop)
                .unwrap();
        }
        if case == 5 {
            next = full
                .window(START + 1, END, 2, ResidentContext::FiniteLoop)
                .unwrap();
        }
        let mut next_stems = stems(&full);
        next_stems.complete_set_identity = old_stems.complete_set_identity.clone();
        next_stems.accepted_timing = Some(accepted());
        if case == 1 {
            next_stems.complete_set_identity = Arc::new([22; 32]);
        }
        let next_stems = next_stems.window_for(&next).unwrap();
        let publication = PreparedSourcePermit::unrestricted();
        publication.mark_pending().unwrap();
        if case == 3 {
            assert!(publication.cancel_unclaimed());
        }
        apply(
            &mut actual,
            ControlMessage::RelocateResident(Box::new(crate::messages::ResidentTransaction {
                id: 0,
                sample: next,
                stems: if case == 4 { None } else { Some(next_stems) },
                binding: before,
                publication: publication.clone(),
                expected_window_revision: if case == 2 { 0 } else { 1 },
                intent: Default::default(),
                seek_pin: None,
            })),
        );
        assert_eq!(publication.status(), "rejected");
        assert_eq!(binding(&actual), before);
        assert_eq!(
            render(&mut actual, 77, 999),
            render(&mut reference, 77, 999)
        );
    }
}

#[test]
fn keylock_enabled_after_window_capture_rejects_finite_relocation_for_active_and_idle_pad() {
    for context in [
        ResidentContext::FiniteLoop,
        ResidentContext::KeyLockFiniteLoop,
    ] {
        for active in [false, true] {
            let full = complete();
            let mut mixer = fixture(&full, None, false);
            if !active {
                mixer.stop_sample(0);
            }
            let captured = binding(&mixer);
            let next = full.window(START, END, 2, context).unwrap();
            // Storage preparation was captured while the full source was dry. The
            // native adoption guard must admit current context even with a live permit.
            let publication = PreparedSourcePermit::unrestricted();
            publication.mark_pending().unwrap();
            mixer.set_pad_key_lock(0, true);
            assert!(mixer.pad_key_lock_enabled[0]);
            let playback = active.then(|| voice(&mixer).source_playback);
            apply(
                &mut mixer,
                ControlMessage::RelocateResident(Box::new(crate::messages::ResidentTransaction {
                    id: 0,
                    sample: next,
                    stems: None,
                    binding: captured,
                    publication: publication.clone(),
                    expected_window_revision: 1,
                    intent: Default::default(),
                    seek_pin: None,
                })),
            );
            assert_eq!(publication.status(), "rejected");
            assert_eq!(binding(&mixer), captured);
            assert_eq!(
                mixer.sample_bank[0].as_ref().unwrap().resident_end(),
                full.frame_count()
            );
            if let Some(playback) = playback {
                assert!(voice(&mixer).source_playback.matches_exact(&playback));
            }
        }
    }
}

#[test]
fn native_keylock_full_context_relocation_keeps_actual_handle_fifo_history_and_output() {
    let full = complete();
    let mut actual = fixture(&full, None, true);
    let mut reference = fixture(&full, None, true);
    assert_eq!(render(&mut actual, 0, 13), render(&mut reference, 0, 13));
    let deadline = Instant::now() + Duration::from_secs(5);
    while (!voice_mut(&mut actual).stretch.source_preparation_ready()
        || !voice_mut(&mut reference).stretch.source_preparation_ready())
        && Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(voice_mut(&mut actual).stretch.source_preparation_ready());
    assert!(voice_mut(&mut reference).stretch.source_preparation_ready());
    assert_eq!(
        render(&mut actual, 13, 4090),
        render(&mut reference, 13, 4090)
    );
    let address = voice(&actual).stretch.native_state_address();
    assert_eq!(voice(&actual).stretch.adopted_request_id(), Some(1));
    let history = voice(&actual).stretch.productive_history().unwrap();
    let mut next = full
        .window(0, full.frame_count(), 2, ResidentContext::KeyLockFullTrack)
        .unwrap();
    next.samples = Arc::from(&full.samples[..]);
    let publication = PreparedSourcePermit::new(
        actual.prepared_source_epochs[0].clone(),
        actual.prepared_source_epochs[0].load(Ordering::Acquire),
    );
    publication.mark_pending().unwrap();
    let before = binding(&actual);
    apply(
        &mut actual,
        ControlMessage::RelocateResident(Box::new(crate::messages::ResidentTransaction {
            id: 0,
            sample: next,
            stems: None,
            binding: before,
            publication: publication.clone(),
            expected_window_revision: 1,
            intent: Default::default(),
            seek_pin: None,
        })),
    );
    assert_eq!(publication.status(), "accepted");
    assert_eq!(voice(&actual).stretch.native_state_address(), address);
    assert_eq!(
        voice(&actual)
            .stretch
            .productive_history()
            .unwrap()
            .fed_output_frames,
        history.fed_output_frames
    );
    let mut elapsed = 4103;
    for frames in [1, 17, 512, 127, 384, 777, 2048] {
        assert_eq!(
            render(&mut actual, elapsed, frames),
            render(&mut reference, elapsed, frames)
        );
        elapsed += frames as u64;
        assert_eq!(voice(&actual).stretch.native_state_address(), address);
        assert_eq!(voice(&actual).stretch.adopted_request_id(), Some(1));
    }
}

#[test]
fn native_keylock_finite_context_relocation_keeps_actual_handle_fifo_history_filter_and_output() {
    for with_stems in [false, true] {
        let full = complete();
        let finite = full
            .window(START, END, 1, ResidentContext::KeyLockFiniteLoop)
            .unwrap();
        let full_stems = with_stems.then(|| stems(&full));
        let finite_stems = full_stems
            .as_ref()
            .map(|set| set.clone().window_for(&finite).unwrap());
        let mut actual = fixture(&finite, finite_stems, true);
        let mut reference = fixture(&full, full_stems.clone(), true);
        assert!(actual.pad_key_lock_enabled[0]);
        assert_eq!(render(&mut actual, 0, 13), render(&mut reference, 0, 13));
        let deadline = Instant::now() + Duration::from_secs(5);
        while (!voice_mut(&mut actual).stretch.source_preparation_ready()
            || !voice_mut(&mut reference).stretch.source_preparation_ready())
            && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(voice_mut(&mut actual).stretch.source_preparation_ready());
        assert!(voice_mut(&mut reference).stretch.source_preparation_ready());
        assert_eq!(
            render(&mut actual, 13, 4090),
            render(&mut reference, 13, 4090)
        );
        let address = voice(&actual).stretch.native_state_address();
        assert_ne!(address, 0);
        assert_eq!(voice(&actual).stretch.adopted_request_id(), Some(1));
        let history = voice(&actual).stretch.productive_history().unwrap();
        let fifos = voice(&actual).stretch.pending_fifo_frames();
        let playback = voice(&actual).source_playback;
        let before = binding(&actual);
        let next = full
            .window(START - 19, END + 29, 2, ResidentContext::KeyLockFiniteLoop)
            .unwrap();
        assert_eq!(next.samples.len(), END - START + 48);
        assert!(!Arc::ptr_eq(&next.samples, &full.samples));
        let next_stems = full_stems.map(|mut set| {
            set.accepted_timing = Some(accepted());
            set.window_for(&next).unwrap()
        });
        let publication = PreparedSourcePermit::new(
            actual.prepared_source_epochs[0].clone(),
            actual.prepared_source_epochs[0].load(Ordering::Acquire),
        );
        publication.mark_pending().unwrap();
        apply(
            &mut actual,
            ControlMessage::RelocateResident(Box::new(crate::messages::ResidentTransaction {
                id: 0,
                sample: next.clone(),
                stems: next_stems,
                binding: before,
                publication: publication.clone(),
                expected_window_revision: 1,
                intent: Default::default(),
                seek_pin: None,
            })),
        );
        assert_eq!(publication.status(), "accepted");
        assert_eq!(voice(&actual).stretch.native_state_address(), address);
        assert_eq!(voice(&actual).stretch.pending_fifo_frames(), fifos);
        assert_eq!(
            voice(&actual)
                .stretch
                .productive_history()
                .unwrap()
                .fed_output_frames,
            history.fed_output_frames
        );
        assert!(voice(&actual).source_playback.matches_exact(&playback));
        assert_eq!(
            voice(&actual).sample.as_ref().unwrap().resident_binding(),
            next.resident_binding()
        );
        // Direct finite seek and active loop edits remain unavailable. Same-resident scalar
        // selection ramps now have their own wet Native/FIFO/filter proof in separate tests.
        let selection = actual.stem_demand_for_measurement(0);
        assert!(!actual.seek_sample_at_output_frame(
            0,
            (START + 37) as f64 / f64::from(RATE),
            4103
        ));
        actual.set_pad_loop_region(
            0,
            START as f64 / f64::from(RATE),
            Some(END as f64 / f64::from(RATE)),
        );
        actual.set_pad_loop_region(
            0,
            (START + 1) as f64 / f64::from(RATE),
            Some((END - 1) as f64 / f64::from(RATE)),
        );
        assert_eq!(actual.loop_region_frames(0), (START, Some(END)));
        assert_eq!(actual.stem_demand_for_measurement(0), selection);
        assert!(voice(&actual).source_playback.matches_exact(&playback));
        assert_eq!(voice(&actual).stretch.native_state_address(), address);
        assert_eq!(voice(&actual).stretch.pending_fifo_frames(), fifos);
        assert_eq!(
            voice(&actual)
                .stretch
                .productive_history()
                .unwrap()
                .fed_output_frames,
            history.fed_output_frames
        );
        let mut elapsed = 4103;
        let mut audible = false;
        for frames in [1, 17, 512, 127, 384, 777, 2048, 31] {
            let output = render(&mut actual, elapsed, frames);
            assert_eq!(output, render(&mut reference, elapsed, frames));
            audible |= output.iter().any(|value| value.abs() > 0.001);
            elapsed += frames as u64;
            assert_eq!(voice(&actual).stretch.native_state_address(), address);
            assert_eq!(voice(&actual).stretch.adopted_request_id(), Some(1));
            assert_eq!(
                voice(&actual).stretch.pending_fifo_frames(),
                voice(&reference).stretch.pending_fifo_frames()
            );
            assert!(
                voice(&actual)
                    .source_playback
                    .matches_exact(&voice(&reference).source_playback)
            );
        }
        assert!(audible, "finite native/filter continuation was silent");
    }
}

#[test]
fn unsupported_physical_controls_and_supported_mode_controls_preserve_finite_metadata_and_audio() {
    let full = complete();
    let finite = full
        .window(START, END, 1, ResidentContext::FiniteLoop)
        .unwrap();
    let mut actual = fixture(&finite, None, false);
    let mut reference = fixture(&full, None, false);
    assert_eq!(finite.frame_count(), 9000);
    assert_eq!(finite.samples.len(), END - START);
    assert_eq!(render(&mut actual, 0, 81), render(&mut reference, 0, 81));
    let before = voice(&actual).source_playback;
    actual.set_pad_loop_region(0, 0.0, None);
    assert!(!actual.seek_sample(0, 0.01));
    assert!(!actual.seek_sample(0, 0.17));
    assert_eq!(actual.loop_region_frames(0), (START, Some(END)));
    assert!(voice(&actual).source_playback.matches_exact(&before));
    assert_eq!(
        render(&mut actual, 81, 1999),
        render(&mut reference, 81, 1999)
    );
    let before_mode = voice(&actual).source_playback;
    let generation = voice(&actual).generation;
    // Actual same-region NormalLoop feed is supported for mode-only scalar/global ON.
    // The complete original PCM retains the same accepted timing and independent storage.
    for mixer in [&mut actual, &mut reference] {
        mixer.set_pad_key_lock(0, true);
        mixer.set_key_lock(true);
        assert!(mixer.pad_key_lock_enabled[0]);
    }
    assert!(voice(&actual).source_playback.matches_exact(&before_mode));
    assert_eq!(voice(&actual).generation, generation);
    let mut frame = 2080;
    for frames in [1, 127, 384, 512, 777, 1024] {
        assert_eq!(
            render(&mut actual, frame, frames),
            render(&mut reference, frame, frames)
        );
        assert!(
            voice(&actual)
                .source_playback
                .matches_exact(&voice(&reference).source_playback)
        );
        assert_eq!(
            voice(&actual).stretch.pending_fifo_frames(),
            voice(&reference).stretch.pending_fifo_frames()
        );
        frame += frames as u64;
    }
    assert_eq!(actual.loop_region_frames(0), (START, Some(END)));
    assert_eq!(voice(&actual).generation, generation);
}

#[test]
fn new_bank_loop_setter_preserves_old_finite_voice_geometry_and_output() {
    let full = complete();
    let finite = full
        .window(START, END, 1, ResidentContext::FiniteLoop)
        .unwrap();
    let mut actual = fixture(&finite, None, false);
    let mut reference = fixture(&finite, None, false);
    assert_eq!(render(&mut actual, 0, 93), render(&mut reference, 0, 93));
    let playback = voice(&actual).source_playback;
    let replacement = complete()
        .window(4000, 5500, 1, ResidentContext::FiniteLoop)
        .unwrap();
    actual.load_sample(0, replacement);
    actual.set_pad_loop_region(0, 4500.0 / f64::from(RATE), Some(5000.0 / f64::from(RATE)));
    assert_eq!(actual.loop_region_frames(0), (4500, Some(5000)));
    assert_eq!(
        voice(&actual).source_loop_region,
        Some(FrameRange {
            start: START,
            end: END
        })
    );
    assert!(voice(&actual).source_playback.matches_exact(&playback));
    let mut elapsed = 93;
    for frames in [1, 127, 512, 999] {
        assert_eq!(
            render(&mut actual, elapsed, frames),
            render(&mut reference, elapsed, frames)
        );
        elapsed += frames as u64;
    }
}

#[test]
fn finite_window_has_no_hidden_complete_pcm_pin_and_old_voice_keeps_source_geometry() {
    let full = complete();
    let weak = Arc::downgrade(&full.samples);
    let finite = full
        .window(START, END, 1, ResidentContext::FiniteLoop)
        .unwrap();
    let mut actual = fixture(&finite, None, false);
    drop(full);
    assert!(weak.upgrade().is_none());
    let before = voice(&actual).source_playback;
    let replacement = complete()
        .window(4000, 5500, 1, ResidentContext::FiniteLoop)
        .unwrap();
    actual.load_sample(0, replacement);
    assert_eq!(
        voice(&actual).source_loop_region,
        Some(FrameRange {
            start: START,
            end: END
        })
    );
    assert!(voice(&actual).source_playback.matches_exact(&before));
    assert!(
        render(&mut actual, 0, 128)
            .iter()
            .any(|value| value.abs() > 0.01)
    );
    assert!(voice(&actual).source_playback.position().frame < END);
}

#[path = "resident_transaction_parity_tests.rs"]
mod resident_transaction_parity_tests;
