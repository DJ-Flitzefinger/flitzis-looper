//! Captured gaps survive same-callback live-original completion and final fences.

use super::*;

fn observed_render(fixture: &mut Fixture, frames: usize) -> Vec<f32> {
    let mut output = vec![0.0; frames];
    let (mut feedback, mut messages) = RingBuffer::new(128);
    let ((), allocations) = observe_current_thread(|| {
        render_scheduled_audio(
            &mut fixture.mixer,
            &mut fixture.scheduler,
            &mut output,
            &mut [0.0; NUM_SAMPLES],
            fixture.transport.output_frame(),
            1,
            &mut fixture.transport,
            &mut feedback,
            &mut fixture.retirement,
        );
    });
    assert_eq!(allocations, ThreadAllocations::default());
    fixture.transport.advance_by_rendered_frames(frames);
    while let Ok(message) = messages.pop() {
        fixture.feedback.push(message);
    }
    output
}

#[test]
fn live_original_chain_preserves_captured_gaps_inside_one_callback_and_full_pcm() {
    for ratio in [0.8, 1.16] {
        for gap in [0, 16] {
            let mut fixture = configured(ratio);
            let first = capture(&fixture, 1.0);
            let gate = WorkerGate::new(&fixture, &first);
            publish(&mut fixture, &first);
            gate.wait_for(false);
            let mut originals = vec![first];
            for ordinal in 1..4 {
                fixture.render_to(gap * ordinal);
                fixture.refresh_clock();
                let next = capture(&fixture, 1.0);
                publish(&mut fixture, &next);
                originals.push(next);
            }
            fixture.render_to(64);
            gate.release();
            assert_eq!(gate.wait_for(true), None);
            wait_ready(
                &mut fixture,
                &originals
                    .iter()
                    .map(CapturedPadStart::sequence)
                    .collect::<Vec<_>>(),
            );
            let mut pcm = observed_render(&mut fixture, 512);
            assert_eq!(fixture.starts(0), 4);
            for (ordinal, start) in originals.iter().enumerate() {
                assert_eq!(start.original_frame(), Some(ordinal as u64 * gap));
                assert_eq!(start.effective_frame(), Some(64 + ordinal as u64 * gap));
                assert_eq!(start.state(), "applied", "{:?}", start.reason());
            }
            assert!(
                originals
                    .windows(2)
                    .all(|pair| pair[0].sequence() < pair[1].sequence())
            );
            assert_eq!(
                fixture.voice(0).stretch.adopted_request_id(),
                Some(originals[3].sequence())
            );
            let last = (3 * gap) as usize;
            pcm.extend(fixture.pcm(last + 24_000 - pcm.len()));
            let reference = physical_birth_tone_reference(24_000, SOURCE_HZ, 0, 0, ratio, true);
            for (frame, actual) in pcm.iter().enumerate() {
                let age = if frame < last && gap != 0 {
                    frame % gap as usize
                } else {
                    frame - last
                };
                assert!(
                    (*actual - reference[age]).abs() < 1.0e-4,
                    "{ratio}/{gap}/{frame}"
                );
            }
        }
    }
}

#[test]
fn live_original_chain_maps_every_old_group_peer_to_its_exact_header() {
    for count in [2, 4, 6] {
        for ratio in [0.8, 1.16] {
            for whole_group in [false, true] {
                let mut fixture = group_fixture(count, ratio);
                let captured = capture_playback_group(&fixture, count);
                fixture.drain();
                let originals = captured
                    .resolve()
                    .unwrap()
                    .unwrap()
                    .into_iter()
                    .map(|member| member.start)
                    .collect::<Vec<_>>();
                let gate = WorkerGate::new(&fixture, &originals[0]);
                assert!(
                    fixture
                        .control
                        .engine
                        .publish_captured_global_start(&captured)
                        .unwrap()
                        .is_some()
                );
                fixture.drain();
                gate.wait_for(false);
                fixture.render(16);
                fixture.refresh_clock();
                let next = if whole_group {
                    playback_group(&mut fixture, count)
                } else {
                    let start = fixture
                        .control
                        .engine
                        .capture_pad_start(count - 1, 0.1, Some(2.1), 1.0, false, None, None)
                        .unwrap();
                    publish(&mut fixture, &start);
                    vec![start]
                };
                fixture.render_to(64);
                gate.release();
                assert_eq!(gate.wait_for(true), None);
                wait_ready(
                    &mut fixture,
                    &originals
                        .iter()
                        .chain(&next)
                        .map(CapturedPadStart::sequence)
                        .collect::<Vec<_>>(),
                );
                observed_render(&mut fixture, 0);
                assert!(
                    originals
                        .iter()
                        .all(|start| start.effective_frame() == Some(64))
                );
                assert!(next.iter().all(|start| start.effective_frame().is_none()
                    && start.execution_frame() == Some(80)));
                let first_native = (0..count)
                    .map(|id| fixture.voice(id).stretch.native_state_address())
                    .collect::<Vec<_>>();
                assert!(
                    first_native
                        .iter()
                        .enumerate()
                        .all(|(i, native)| *native != 0 && !first_native[..i].contains(native))
                );
                let mut pcm = observed_render(&mut fixture, 512);
                assert!(next.iter().all(|start| start.effective_frame() == Some(80)));
                assert_eq!(fixture.scheduler.len(), 0);
                for (member, start) in next.iter().enumerate() {
                    let id = if whole_group { member } else { count - 1 };
                    assert_eq!(fixture.starts(id), 2);
                    assert_eq!(
                        fixture.voice(id).stretch.adopted_request_id(),
                        Some(start.sequence())
                    );
                    assert!(
                        !first_native.contains(&fixture.voice(id).stretch.native_state_address())
                    );
                }
                pcm.extend(fixture.pcm(24_016 - pcm.len()));
                let reference = physical_birth_tone_reference(24_016, SOURCE_HZ, 0, 0, ratio, true);
                for (frame, actual) in pcm.iter().enumerate() {
                    let expected = if frame < 16 {
                        reference[frame] * count as f32
                    } else if whole_group {
                        reference[frame - 16] * count as f32
                    } else {
                        reference[frame] * (count - 1) as f32 + reference[frame - 16]
                    };
                    assert!(
                        (*actual - expected).abs() < 1.0e-4,
                        "{count}/{ratio}/{whole_group}/{frame}"
                    );
                }
            }
        }
    }
}

#[test]
fn live_original_chain_cannot_cross_a_final_feedback_deferral() {
    let mut fixture = configured(0.8);
    let first = capture(&fixture, 1.0);
    let gate = WorkerGate::new(&fixture, &first);
    publish(&mut fixture, &first);
    gate.wait_for(false);
    fixture.render(16);
    fixture.refresh_clock();
    let next = capture(&fixture, 1.0);
    publish(&mut fixture, &next);
    fixture.render_to(64);
    gate.release();
    assert_eq!(gate.wait_for(true), None);
    wait_ready(&mut fixture, &[first.sequence(), next.sequence()]);
    let (mut feedback, mut messages) = RingBuffer::new(1);
    feedback
        .push(AudioMessage::SampleStarted { id: 1 })
        .unwrap();
    let mut output = [0.0; 512];
    render_scheduled_audio(
        &mut fixture.mixer,
        &mut fixture.scheduler,
        &mut output,
        &mut [0.0; NUM_SAMPLES],
        64,
        1,
        &mut fixture.transport,
        &mut feedback,
        &mut fixture.retirement,
    );
    assert!(output.iter().all(|value| *value == 0.0));
    assert_eq!(first.execution_frame(), Some(64));
    assert_eq!(next.execution_frame(), Some(80));
    assert!(first.effective_frame().is_none() && next.effective_frame().is_none());
    assert!(fixture.scheduler.deferred_due().is_some());
    messages.pop().unwrap();
    fixture.transport.advance_by_rendered_frames(512);
    fixture.render(0);
    assert!(first.effective_frame().is_none() && next.effective_frame().is_none());
    assert!(first.execution_frame().unwrap() >= 8192);
    assert!(next.execution_frame().unwrap() >= first.execution_frame().unwrap());
    wait_ready(&mut fixture, &[first.sequence(), next.sequence()]);
    fixture.render(1);
    assert_eq!(first.effective_frame(), Some(576));
    assert!(next.effective_frame().is_none());
    fixture.render(16);
    assert_eq!(next.effective_frame(), Some(592));
    assert_eq!(fixture.starts(0), 2);
}
