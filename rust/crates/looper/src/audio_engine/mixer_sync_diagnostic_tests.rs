use super::*;
use crate::audio_engine::sync_diagnostics::{HEADER_WORDS, ROW_WORDS};
use crate::audio_engine::transport::TransportTimeline;
use crate::test_memory::{ThreadAllocations, observe_current_thread};

#[test]
fn sync_diagnostic_legacy_eight_and_sixty_four_bar_stems_share_boundary() {
    let mut mixer = RtMixer::try_new(1, 8_000.0, None).unwrap();
    let frames = 64 * 4 * 4_000;
    let source = create_test_sample(1, frames, 0.25);
    mixer.load_sample(1, source.clone()); mixer.load_sample(2, source.clone());
    assert!(mixer.publish_prepared_stems(2, create_test_prepared_stems(&source, 1, 8_000, frames)));
    assert!(mixer.set_stem_mix_mode(2, StemMixMode::AllStems, 42));
    mixer.set_pad_loop_region(1, 0.0, Some(16.0)); mixer.set_pad_loop_region(2, 0.0, Some(128.0));
    assert!(mixer.play_sample(1, 1.0)); assert!(mixer.play_sample(2, 1.0));
    let mut timeline = TransportTimeline::new(8_000);
    let mut out = [0.0; 512]; let mut peaks = [0.0; NUM_SAMPLES];
    for _ in 0..251 { mixer.render_at_output_frame(timeline.output_frame(), &mut out, &mut peaks); timeline.advance_by_rendered_frames(512); }
    mixer.begin_sync_diagnostic_capture(9);
    let (snapshot, counts) = observe_current_thread(|| {
        mixer.render_at_output_frame(timeline.output_frame(), &mut out, &mut peaks);
        timeline.advance_by_rendered_frames(512);
        mixer.sync_diagnostic_snapshot(9, 3, &timeline, 77, 512).unwrap()
    });
    assert_eq!(counts, ThreadAllocations::default());
    assert!(out.iter().any(|value| *value != 0.0));
    assert_eq!(snapshot.0[3], 252 * 512); assert_eq!(snapshot.0[11], 2);
    let rows = &snapshot.0[HEADER_WORDS..];
    for row in rows.chunks_exact(ROW_WORDS).take(2) {
        assert_eq!(row[30], snapshot.0[3]); assert_eq!(row[33], 1);
        assert_ne!(row[16] & (1 << 7), 0); assert_eq!(row[16] & (1 << 5), 0);
        assert_eq!(row[8], row[31]);
    }
    assert_eq!(rows[11], 128_000); assert_eq!(rows[ROW_WORDS + 11], 1_024_000);
    assert_eq!(rows[ROW_WORDS + 36], 1); assert_eq!(rows[ROW_WORDS + 37], 42);
    mixer.end_sync_diagnostic_capture();
    assert!(mixer.sync_diagnostic_snapshot(9, 3, &timeline, 77, 512).is_none());
    mixer.begin_sync_diagnostic_capture(10); mixer.pause_sample(2);
    mixer.render_at_output_frame(timeline.output_frame(), &mut out, &mut peaks); timeline.advance_by_rendered_frames(512);
    let paused = mixer.sync_diagnostic_snapshot(10, 3, &timeline, 78, 512).unwrap();
    assert_eq!(paused.0[HEADER_WORDS + ROW_WORDS + 16] & (1 << 7), 0);
}

#[test]
fn sync_diagnostic_native_input_is_separate_from_logical_cursor_and_stale_read() {
    let mut mixer = RtMixer::try_new(1, 48_000.0, None).unwrap();
    mixer.load_sample(0, create_sine_sample(48_000.0, 96_000, 440.0));
    mixer.set_speed(1.5); mixer.set_key_lock(true); assert!(mixer.play_sample(0, 1.0));
    let mut timeline = TransportTimeline::new(48_000);
    let mut out = [0.0; 512]; let mut peaks = [0.0; NUM_SAMPLES];
    for _ in 0..32 { mixer.render_at_output_frame(timeline.output_frame(), &mut out, &mut peaks); timeline.advance_by_rendered_frames(512); }
    mixer.begin_sync_diagnostic_capture(1);
    mixer.render_at_output_frame(timeline.output_frame(), &mut out, &mut peaks); timeline.advance_by_rendered_frames(512);
    let snapshot = mixer.sync_diagnostic_snapshot(1, 1, &timeline, 1, 512).unwrap();
    let row = &snapshot.0[HEADER_WORDS..];
    assert_ne!(row[16] & (1 << 7), 0); assert_eq!(row[33], 2); assert_eq!(row[30], timeline.output_frame());
    timeline.advance_by_rendered_frames(1);
    let stale = mixer.sync_diagnostic_snapshot(1, 1, &timeline, 1, 1).unwrap();
    assert_eq!(stale.0[HEADER_WORDS + 16] & (1 << 7), 0);
}
