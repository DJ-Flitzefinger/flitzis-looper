// The publication setup is shared; MODE's chronology, selection arithmetic and
// direct-C-ABI PCM reference remain independent from production source readers.
mod selection_mode_history_tests {
    use super::*;
    use crate::audio_engine::pitch_runtime_tests::source_feed_tests::selection_source_feed_tests::selection_setup;

    #[derive(Clone, Copy, Debug)]
    struct SelectionFrame {
        from: Option<u8>,
        to: Option<u8>,
        progress: f64,
    }

    fn normal_timeline(frames: usize) -> Vec<ScalarFrame> {
        let mut origin_frame = START;
        let mut origin_fraction = 0.0;
        let mut epoch = 0;
        let mut rate: f64 = 1.0;
        let mut target: f64 = 1.0;
        let mut next_step = usize::MAX;
        let mut result = Vec::with_capacity(frames + 1);
        for active in 0..=frames {
            if let Some((_, value)) = EVENTS.iter().find(|(at, _)| *at == active) {
                target = *value;
                next_step = active;
            }
            if active == next_step && rate != target {
                let distance = origin_fraction + (active - epoch) as f64 * rate;
                let whole = distance.floor() as usize;
                origin_frame = START + (origin_frame - START + whole) % (END - START);
                origin_fraction = distance - whole as f64;
                epoch = active;
                rate = if (target - rate).abs() <= 0.05 {
                    target
                } else {
                    rate + (target - rate).clamp(-0.05, 0.05)
                };
                next_step = active + BLOCK;
            }
            let distance = origin_fraction + (active - epoch) as f64 * rate;
            let whole = distance.floor() as usize;
            result.push(ScalarFrame {
                frame: START + (origin_frame - START + whole) % (END - START),
                fraction: distance - whole as f64,
                passage: Passage::Normal,
                rate,
            });
        }
        result
    }

    fn selection_events(full_mix_e: bool) -> [(usize, Option<u8>); 5] {
        [
            (F - 5_500, Some(0b1111)),
            (F - 4_200, Some(0b0101)),
            (F - 3_000, Some(0)),
            (F - 1_800, if full_mix_e { Some(0b1010) } else { None }),
            (F - 43, if full_mix_e { None } else { Some(0b1010) }),
        ]
    }

    fn selection_timeline(timeline: &[ScalarFrame], full_mix_e: bool) -> Vec<SelectionFrame> {
        let events = selection_events(full_mix_e);
        let mut from = None;
        let mut to = None;
        let mut progress = 128.0;
        timeline
            .iter()
            .enumerate()
            .map(|(active, frame)| {
                if let Some((_, next)) = events.iter().find(|(at, _)| *at == active) {
                    assert!(
                        progress >= 128.0,
                        "the independent selections never overlap"
                    );
                    from = to;
                    to = *next;
                    progress = 0.0;
                }
                let selection = SelectionFrame { from, to, progress };
                progress += frame.rate;
                selection
            })
            .collect()
    }

    fn select(fixture: &mut Fixture, stems: &PreparedStemSet, selection: Option<u8>) {
        if let Some(mask) = selection {
            assert!(
                fixture
                    .mixer
                    .set_stem_enabled_mask(0, mask, stems.source_version_hash)
            );
            assert!(fixture.mixer.set_stem_mix_mode(
                0,
                StemMixMode::AllStems,
                stems.source_version_hash
            ));
        } else {
            assert!(fixture.mixer.set_stem_mix_mode(0, StemMixMode::FullMix, 0));
        }
    }

    fn controls(fixture: &mut Fixture, stems: &PreparedStemSet, origin: u64, full_mix_e: bool) {
        for (event, rate) in EVENTS.into_iter().take(3) {
            fixture.render_to(origin + event as u64);
            if event == EVENTS[0].0 {
                fixture.mixer.set_speed(rate);
            } else {
                if event == EVENTS[1].0 {
                    fixture.mixer.set_bpm_lock(true);
                }
                fixture.mixer.set_master_period(0.5 / rate);
            }
        }
        // The real initial seek is an ownership cut. Resynchronize through an
        // original nonstarting SET before any of the selection events below.
        fixture.render_to(origin + 54_000);
        let pitch = fixture.mixer.pad_pitch_intent[0];
        let old = fixture.mixer.pad_key_lock_enabled[0];
        fixture.rows(PitchPadProjection {
            intent: pitch,
            key_lock: old,
            ..PitchPadProjection::default()
        });
        let resync = fixture.capture(0, &format!("pitch.extra:{}:set", pitch.extra_shift()));
        fixture.publish(&resync);
        fixture.wait_worker(&[&resync]);
        fixture.render_to(resync.execution_target());
        fixture.render(1);
        assert!(
            resync.effective(),
            "{} {:?}",
            resync.state(),
            resync.reason()
        );
        assert!(fixture.voice(0).stretch.owns_pitch_contract(pitch, old));
        for (event, selection) in selection_events(full_mix_e).into_iter().take(4) {
            fixture.render_to(origin + event as u64);
            select(fixture, stems, selection);
        }
        fixture.render_to(origin + EVENTS[3].0 as u64);
        fixture.mixer.set_master_period(0.5 / EVENTS[3].1);
        let (event, selection) = selection_events(full_mix_e)[4];
        fixture.render_to(origin + event as u64);
        select(fixture, stems, selection);
        fixture.render_to(origin + F as u64);
        assert!(fixture.mixer.stem_transitions[0].is_active());
        assert!(fixture.voice(0).stretch.owns_pitch_contract(pitch, old));
    }

    fn scalar_pcm(
        source: &SampleBuffer,
        stems: &PreparedStemSet,
        frame: ScalarFrame,
        selection: SelectionFrame,
    ) -> f32 {
        let selected = |at: usize, mask: Option<u8>| {
            mask.map_or_else(
                || source.samples[at - source.resident_start()],
                |mask| {
                    stems
                        .stems
                        .iter()
                        .enumerate()
                        .filter(|(index, _)| mask & (1 << index) != 0)
                        .map(|(_, stem)| stem.samples[at - stem.resident_start()])
                        .sum::<f32>()
                },
            )
        };
        let mixed = |at| {
            if selection.progress < 128.0 {
                let to = selection.progress as f32 / 128.0;
                selected(at, selection.from) * (1.0 - to) + selected(at, selection.to) * to
            } else {
                selected(at, selection.to)
            }
        };
        let right = if frame.frame + 1 == END {
            START
        } else {
            frame.frame + 1
        };
        let left = mixed(frame.frame);
        left + (mixed(right) - left) * frame.fraction as f32
    }

    fn ack(fixture: &Fixture) -> u64 {
        fixture
            .mixer
            .current_timing_acknowledgements
            .current_for_slot(&fixture.mixer.input_runtime_ownership, 0)
    }

    #[test]
    fn held_same_set_mode_replays_masks_empty_mask_and_both_fullmix_ramps_at_original_e() {
        for pitch in [
            PitchIntent::new(6, 18).unwrap(),
            PitchIntent::new(-5, -18).unwrap(),
        ] {
            for old in [false, true] {
                for full_mix_e in [false, true] {
                    let (mut fixture, source, stems, origin) = selection_setup(old, pitch);
                    controls(&mut fixture, &stems, origin, full_mix_e);
                    let timeline = normal_timeline(E + 4_097);
                    let selections = selection_timeline(&timeline, full_mix_e);
                    assert_phase(&fixture, timeline[F]);
                    assert!(selections[F].progress < 128.0);
                    assert!(selections[E].progress > 128.0);
                    let total = pitch.total_semitones();
                    let (reset_origin, bound) = bounded_origin(&timeline, E, total, !old);
                    assert!(bound > E - F && reset_origin < F - 5_500);
                    let first_pitch = timeline[reset_origin + BLOCK - 1].pitch(total, !old);
                    let mut reference = DirectNative::new(first_pitch);
                    let mut wrong_fullmix = DirectNative::new(first_pitch);
                    for active in reset_origin..E {
                        let frame = timeline[active];
                        reference.sample(
                            scalar_pcm(&source, &stems, frame, selections[active]),
                            frame.pitch(total, !old),
                        );
                        wrong_fullmix.sample(
                            scalar_pcm(
                                &source,
                                &stems,
                                frame,
                                SelectionFrame {
                                    from: None,
                                    to: None,
                                    progress: 128.0,
                                },
                            ),
                            frame.pitch(total, !old),
                        );
                    }
                    let generation = fixture.voice(0).generation;
                    let address = fixture.voice(0).sample.as_ref().unwrap().source_address();
                    let old_native = fixture.voice(0).stretch.native_state_address();
                    let lane = &fixture.voice(0).stretch as *const _ as usize;
                    let starts = fixture.starts(0);
                    let original_ack = ack(&fixture);
                    wait_history_capacity(&mut fixture, "held same-set mode capture");
                    let request = submit_scalar(&mut fixture, !old);
                    let pending = fixture.mixer.pending_key_lock_tuples[0].as_ref().unwrap();
                    assert_eq!(pending.request.request_id, request);
                    assert_eq!(pending.request.pitch, pitch);
                    assert_eq!(pending.execution, Some(origin + E as u64));
                    let binding = pending.request.binding;
                    let source_generation = pending.request.source_generation;
                    wait_own_candidate(&mut fixture, request, "held same-set mode preparation");
                    advance_old_to(
                        &mut fixture,
                        origin + E as u64,
                        origin,
                        &timeline,
                        old,
                        old_native,
                    );
                    assert!(!fixture.mixer.stem_transitions[0].is_active());
                    let pending = fixture.mixer.pending_key_lock_tuples[0].as_ref().unwrap();
                    assert_eq!(pending.request.binding, binding);
                    assert_eq!(pending.request.source_generation, source_generation);
                    assert_eq!(pending.execution, Some(origin + E as u64));
                    let mut filter = fixture.mixer.pad_dsp_chains[0].clone();
                    let mut cold = filter.clone();
                    cold.reset();
                    assert!(
                        filter_history_probe(&filter)
                            .iter()
                            .zip(filter_history_probe(&cold))
                            .any(|(warm, cold)| (*warm - cold).abs() > 1.0e-7)
                    );
                    let mut adopted_native = None;
                    let mut maximum = 0.0_f32;
                    let mut wrong_maximum = 0.0_f32;
                    for frames in PARTITIONS {
                        let active = (fixture.transport.output_frame() - origin) as usize;
                        let pcm = fixture.render(frames);
                        let native = fixture.voice(0).stretch.native_state_address();
                        assert_ne!(native, old_native);
                        assert_eq!(
                            *adopted_native.get_or_insert(native),
                            native,
                            "adopted MODE owner must not be replaced automatically"
                        );
                        for (offset, actual) in pcm.iter().enumerate() {
                            let at = active + offset;
                            let frame = timeline[at];
                            let input = scalar_pcm(&source, &stems, frame, selections[at]);
                            let expected = reference.sample(input, frame.pitch(total, !old));
                            let wrong = wrong_fullmix.sample(input, frame.pitch(total, !old));
                            let error = (*actual - expected).abs();
                            maximum = maximum.max(error);
                            wrong_maximum = wrong_maximum.max((*actual - wrong).abs());
                            assert!(
                                error < 2.0e-6,
                                "k={total} old={old} FullMix_E={full_mix_e} adopted_offset={} actual={actual} independent_A_reset={expected} error={error}",
                                at - E
                            );
                            filter.begin_frame();
                            filter.process_sample(0, expected);
                        }
                        assert_phase(&fixture, timeline[active + frames]);
                        assert_eq!(
                            fixture.voice(0).stretch.pending_fifo_frames(),
                            reference.fifo()
                        );
                        assert_eq!(fixture.voice(0).generation, generation);
                        assert_eq!(
                            fixture.voice(0).sample.as_ref().unwrap().source_address(),
                            address
                        );
                        assert_eq!(&fixture.voice(0).stretch as *const _ as usize, lane);
                        assert_eq!(fixture.mixer.pad_pitch_intent[0], pitch);
                        assert_eq!(fixture.mixer.pad_key_lock_enabled[0], !old);
                        assert_eq!(ack(&fixture), original_ack);
                        let status = fixture.mixer.pad_key_lock_status[0].unwrap();
                        assert_eq!(status.request_id, request);
                        assert!(status.ready);
                        let held = fixture.mixer.prepared_stems[0].as_ref().unwrap();
                        assert!(Arc::ptr_eq(
                            &held.complete_set_identity,
                            &stems.complete_set_identity
                        ));
                        assert!(Arc::ptr_eq(
                            &held.reference_samples,
                            &stems.reference_samples
                        ));
                        assert_eq!(held.publication.expected, stems.publication.expected);
                        assert!(held.publication.current());
                    }
                    assert!(
                        filter_history_probe(&fixture.mixer.pad_dsp_chains[0])
                            .iter()
                            .zip(filter_history_probe(&filter))
                            .all(|(actual, expected)| (*actual - expected).abs() < 2.0e-6),
                        "MODE must preserve the existing filter chronology"
                    );
                    assert!(
                        wrong_maximum > 0.005,
                        "wrong FullMix-only history must audibly disagree even for FullMix E: k={total} old={old} FullMix_E={full_mix_e} error={wrong_maximum}"
                    );
                    assert_eq!(fixture.starts(0), starts);
                    assert!(fixture.mixer.pending_key_lock_tuples[0].is_none());
                    let status = fixture.mixer.pad_key_lock_status[0].unwrap();
                    assert_eq!(status.request_id, request);
                    assert!(status.ready);
                    assert_eq!(status.effective, !old);
                    assert_eq!(
                        fixture
                            .voice(0)
                            .stretch
                            .productive_history()
                            .unwrap()
                            .fed_output_frames,
                        (E - reset_origin + 4_097) as u64
                    );
                    println!(
                        "held_selection_prospective_mode_A_reset k={total} old={old} FullMix_E={full_mix_e} O={origin} A={reset_origin} F={F} E={E} H={bound} every4097_max_abs={maximum} wrong_FullMix_history_max_abs={wrong_maximum}; continuous_O_and_acoustic_acceptance_are_separate"
                    );
                }
            }
        }
    }

    #[test]
    fn held_selection_or_source_aba_refuses_original_mode_then_same_lane_accepts_fresh_request() {
        for selection_aba in [false, true] {
            let pitch = PitchIntent::new(6, 18).unwrap();
            let (mut fixture, _source, stems, origin) = selection_setup(false, pitch);
            controls(&mut fixture, &stems, origin, true);
            let timeline = normal_timeline(E + 16_384);
            let native = fixture.voice(0).stretch.native_state_address();
            let lane = &fixture.voice(0).stretch as *const _ as usize;
            let generation = fixture.voice(0).generation;
            let address = fixture.voice(0).sample.as_ref().unwrap().source_address();
            let starts = fixture.starts(0);
            let original_ack = ack(&fixture);
            let refused = submit_scalar(&mut fixture, true);
            wait_own_candidate(&mut fixture, refused, "held ABA original mode");
            let original_e = fixture.mixer.pending_key_lock_tuples[0]
                .as_ref()
                .unwrap()
                .execution
                .unwrap();
            assert_eq!(original_e, origin + E as u64);
            if selection_aba {
                // Both genuine controls occur between reads and restore FullMix E.
                select(&mut fixture, &stems, Some(0b0101));
                select(&mut fixture, &stems, None);
            } else {
                fixture.mixer.set_master_period(0.5 / 0.91);
                fixture.mixer.set_master_period(0.5 / EVENTS[3].1);
            }
            advance_old_to(
                &mut fixture,
                original_e + 1,
                origin,
                &timeline,
                false,
                native,
            );
            assert!(fixture.mixer.pending_key_lock_tuples[0].is_none());
            let status = fixture.mixer.pad_key_lock_status[0].unwrap();
            assert_eq!(status.request_id, refused);
            assert_eq!(status.state, "error");
            assert!(!status.ready && !status.effective);
            assert!(!fixture.voice(0).stretch.wet_mode_failed());
            let fresh_f = fixture.transport.output_frame();
            wait_history_capacity(&mut fixture, "held ABA fresh mode");
            let fresh = submit_scalar(&mut fixture, true);
            assert!(fresh > refused);
            wait_own_candidate(&mut fixture, fresh, "held ABA fresh mode preparation");
            let fresh_e = fixture.mixer.pending_key_lock_tuples[0]
                .as_ref()
                .unwrap()
                .execution
                .unwrap();
            assert_eq!(fresh_e, fresh_f + 4_096);
            advance_old_to(&mut fixture, fresh_e, origin, &timeline, false, native);
            fixture.render(1);
            assert_ne!(fixture.voice(0).stretch.native_state_address(), native);
            assert_eq!(fixture.voice(0).generation, generation);
            assert_eq!(
                fixture.voice(0).sample.as_ref().unwrap().source_address(),
                address
            );
            assert_eq!(&fixture.voice(0).stretch as *const _ as usize, lane);
            assert_eq!(fixture.starts(0), starts);
            assert_eq!(ack(&fixture), original_ack);
            assert_eq!(fixture.mixer.pad_pitch_intent[0], pitch);
            let status = fixture.mixer.pad_key_lock_status[0].unwrap();
            assert_eq!(status.request_id, fresh);
            assert!(status.ready && status.effective);
        }
    }

    #[test]
    fn paused_mid_selection_ramp_refuses_mode_at_active_e_without_consuming_old_owner() {
        let pitch = PitchIntent::new(-5, -18).unwrap();
        let (mut fixture, _source, stems, origin) = selection_setup(false, pitch);
        controls(&mut fixture, &stems, origin, false);
        pause_public(&mut fixture);
        let phase = fixture.voice(0).source_playback;
        let native = fixture.voice(0).stretch.native_state_address();
        let fifo = fixture.voice(0).stretch.pending_fifo_frames();
        let generation = fixture.voice(0).generation;
        let starts = fixture.starts(0);
        let filter = filter_history_probe(&fixture.mixer.pad_dsp_chains[0]);
        let request = submit_scalar(&mut fixture, true);
        let pcm = fixture.render_to(origin + E as u64 + 1);
        assert!(pcm.iter().all(|sample| *sample == 0.0));
        assert!(fixture.mixer.pending_key_lock_tuples[0].is_none());
        let status = fixture.mixer.pad_key_lock_status[0].unwrap();
        assert_eq!(status.request_id, request);
        assert_eq!(status.state, "error");
        assert!(!status.ready && !status.effective);
        assert!(!fixture.mixer.pad_key_lock_enabled[0]);
        assert!(fixture.mixer.stem_transitions[0].is_active());
        assert!(fixture.voice(0).source_playback.matches_exact(&phase));
        assert_eq!(fixture.voice(0).stretch.native_state_address(), native);
        assert_eq!(fixture.voice(0).stretch.pending_fifo_frames(), fifo);
        assert_eq!(fixture.voice(0).generation, generation);
        assert_eq!(fixture.starts(0), starts);
        assert_eq!(
            filter_history_probe(&fixture.mixer.pad_dsp_chains[0]),
            filter
        );
        assert!(!fixture.voice(0).stretch.wet_mode_failed());
    }
}
