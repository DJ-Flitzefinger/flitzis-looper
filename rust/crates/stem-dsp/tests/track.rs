use flitzis_stem_dsp::{
    Error, RATE,
    track::{BATCH, CHUNK, RING, STEP, TrackPlan, process_track},
};
use std::cell::Cell;

fn sample(channel: usize, frame: usize) -> f32 {
    ((frame * 17 % 251) as f32 - 125.0) * 0.001 + channel as f32 * 0.125
}
fn read(start: usize, count: usize, output: &mut [f32]) -> Result<usize, Error> {
    for channel in 0..2 {
        for i in 0..count {
            output[channel * count + i] = sample(channel, start + i);
        }
    }
    Ok(count)
}
fn copy_prediction(input: &[f32], batch: usize) -> Result<Vec<f32>, Error> {
    let mut output = vec![0.0; batch * 8 * CHUNK];
    for b in 0..batch {
        for stem in 0..4 {
            output[(b * 4 + stem) * 2 * CHUNK..(b * 4 + stem + 1) * 2 * CHUNK]
                .copy_from_slice(&input[b * 2 * CHUNK..(b + 1) * 2 * CHUNK]);
        }
    }
    Ok(output)
}

#[test]
fn geometry_is_checked_before_any_payload_and_original_budget_is_unchanged() {
    for frames in [0, usize::MAX, usize::MAX / 8 + 1] {
        assert!(matches!(TrackPlan::new(frames, RATE), Err(Error::Shape)));
    }
    assert!(matches!(TrackPlan::new(100, 48_000), Err(Error::Shape)));
    assert_eq!(TrackPlan::new(CHUNK, RATE).unwrap().border(), 0);
    let p = TrackPlan::new(CHUNK + 1, RATE).unwrap();
    assert_eq!(p.border(), STEP);
    assert_eq!(p.virtual_frames(), 2 * CHUNK + 1);
    assert_eq!(p.chunks(), 5);
    assert_eq!(p.pcm_transient_bytes(), 183_367_800);
    assert_eq!(p.core_pcm_payload_bytes(), 86_347_800);
    assert!(p.core_pcm_payload_bytes() < p.pcm_transient_bytes());
    assert_eq!(RING, 727_650);
    assert_eq!(BATCH, 2);
}

#[test]
fn complete_source_reflection_tail_thresholds_batch_order_and_crop() {
    // Includes exact border and final-reflection thresholds on BOTH sides.
    for frames in [
        1,
        1025,
        STEP,
        STEP + 1,
        STEP + 2,
        CHUNK,
        CHUNK + 1,
        CHUNK + 2,
        CHUNK + 3,
    ] {
        let plan = TrackPlan::new(frames, RATE).unwrap();
        let mut ordinal = 0;
        let mut written = 0;
        process_track(
            plan,
            read,
            |input, batch| {
                assert_eq!(batch, (plan.chunks() - ordinal).min(2));
                for b in 0..batch {
                    let start = (ordinal + b) * STEP;
                    let length = CHUNK.min(plan.virtual_frames() - start);
                    for channel in 0..2 {
                        for i in 0..CHUNK {
                            let local = if i < length {
                                Some(i)
                            } else if length > STEP + 1 {
                                Some(2 * length - 2 - i)
                            } else {
                                None
                            };
                            let expected = local.map_or(0.0, |local| {
                                let v = start + local;
                                let physical = if v < plan.border() {
                                    plan.border() - v
                                } else if v < plan.border() + frames {
                                    v - plan.border()
                                } else {
                                    frames - 2 - (v - plan.border() - frames)
                                };
                                sample(channel, physical)
                            });
                            assert_eq!(
                                input[(b * 2 + channel) * CHUNK + i],
                                expected,
                                "frames={frames} chunk={} channel={channel} local={i}",
                                ordinal + b
                            );
                        }
                    }
                }
                ordinal += batch;
                copy_prediction(input, batch)
            },
            |start, count, output| {
                assert_eq!(start, written);
                assert!(count <= CHUNK);
                for lane in 0..8 {
                    for i in 0..count {
                        assert!(
                            (output[lane * count + i] - sample(lane % 2, start + i)).abs() < 6e-8
                        );
                    }
                }
                written += count;
                Ok(())
            },
            &|| false,
        )
        .unwrap();
        assert_eq!(ordinal, plan.chunks());
        assert_eq!(written, frames);
    }
}

#[test]
fn source_and_complete_prediction_failures_are_terminal_before_first_output() {
    let p = TrackPlan::new(1, RATE).unwrap();
    let written = Cell::new(false);
    let sink = |_: usize, _: usize, _: &[f32]| {
        written.set(true);
        Ok(())
    };
    assert_eq!(
        process_track(p, |_, _, _| Ok(0), copy_prediction, sink, &|| false),
        Err(Error::Source)
    );
    assert_eq!(
        process_track(
            p,
            |_, _, _| Err(Error::Source),
            copy_prediction,
            sink,
            &|| false
        ),
        Err(Error::Source)
    );
    assert_eq!(
        process_track(
            p,
            |_, count, out| {
                out.fill(f32::NAN);
                Ok(count)
            },
            copy_prediction,
            sink,
            &|| false
        ),
        Err(Error::NonFinite)
    );
    assert_eq!(
        process_track(p, read, |_, _| Ok(vec![0.0; 8 * CHUNK - 1]), sink, &|| {
            false
        }),
        Err(Error::Prediction)
    );
    assert_eq!(
        process_track(
            p,
            read,
            |_, _| {
                let mut out = vec![0.0; 8 * CHUNK];
                *out.last_mut().unwrap() = f32::INFINITY;
                Ok(out)
            },
            sink,
            &|| false
        ),
        Err(Error::NonFinite)
    );
    assert!(!written.get());
}

#[test]
fn finite_prediction_overflow_and_late_sink_failure_never_succeed() {
    let p = TrackPlan::new(CHUNK, RATE).unwrap();
    assert_eq!(
        process_track(
            p,
            read,
            |_, batch| Ok(vec![f32::MAX; batch * 8 * CHUNK]),
            |_, _, _| panic!("overflow must precede flush"),
            &|| false
        ),
        Err(Error::NonFinite)
    );
    let blocks = Cell::new(0);
    let p = TrackPlan::new(3 * CHUNK + 1, RATE).unwrap();
    assert_eq!(
        process_track(
            p,
            read,
            copy_prediction,
            |_, _, _| {
                blocks.set(blocks.get() + 1);
                if blocks.get() == 2 {
                    Err(Error::Output)
                } else {
                    Ok(())
                }
            },
            &|| false
        ),
        Err(Error::Output)
    );
    assert_eq!(blocks.get(), 2);
}

#[test]
fn cancellation_before_source_after_model_during_accumulation_and_after_sink() {
    let p = TrackPlan::new(CHUNK + 1, RATE).unwrap();
    assert_eq!(
        process_track(
            p,
            |_, _, _| panic!("cancel before read"),
            copy_prediction,
            |_, _, _| Ok(()),
            &|| true
        ),
        Err(Error::Cancelled)
    );
    let cancel = Cell::new(false);
    assert_eq!(
        process_track(
            p,
            |start, n, out| {
                let result = read(start, n, out);
                cancel.set(true);
                result
            },
            |_, _| panic!("cancel before model"),
            |_, _, _| Ok(()),
            &|| cancel.get()
        ),
        Err(Error::Cancelled)
    );
    cancel.set(false);
    assert_eq!(
        process_track(
            p,
            read,
            |input, batch| {
                let out = copy_prediction(input, batch);
                cancel.set(true);
                out
            },
            |_, _, _| panic!("cancel before output"),
            &|| cancel.get()
        ),
        Err(Error::Cancelled)
    );
    // Arm cancellation after prediction validation, then cross into accumulation.
    let calls = Cell::new(0);
    let armed = Cell::new(false);
    assert_eq!(
        process_track(
            p,
            read,
            |input, batch| {
                armed.set(true);
                copy_prediction(input, batch)
            },
            |_, _, _| panic!("cancel in accumulation"),
            &|| {
                if armed.get() {
                    calls.set(calls.get() + 1);
                }
                calls.get() > (2 * 8 * CHUNK).div_ceil(4096) + 10
            }
        ),
        Err(Error::Cancelled)
    );
    // Cross full prediction validation and both complete accumulation loops;
    // cancellation then occurs while normalizing the first physical flush.
    calls.set(0);
    armed.set(false);
    let normalize_after = 1 + (2 * 8 * CHUNK).div_ceil(4096) + 2 * CHUNK.div_ceil(4096) + 10;
    assert_eq!(
        process_track(
            p,
            read,
            |input, batch| {
                armed.set(true);
                copy_prediction(input, batch)
            },
            |_, _, _| panic!("cancel in normalization"),
            &|| {
                if armed.get() {
                    calls.set(calls.get() + 1);
                }
                calls.get() > normalize_after
            }
        ),
        Err(Error::Cancelled)
    );
    cancel.set(false);
    assert_eq!(
        process_track(
            p,
            read,
            copy_prediction,
            |_, _, _| {
                cancel.set(true);
                Ok(())
            },
            &|| cancel.get()
        ),
        Err(Error::Cancelled)
    );
}
