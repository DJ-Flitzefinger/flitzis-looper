use super::*;
use crate::audio_engine::cold_store::descriptor_digest;
use serde_json::{Value, json};

const RATE: u32 = 100;
const FRAMES: usize = 10_000;

fn bits(value: f64) -> String {
    format!("{:016x}", value.to_bits())
}

fn map(anchors: &[(f64, f64)], coverage: &[&str], verified_meter: bool) -> SourceBeatmap {
    let decoder = json!({
        "schema_version": 1, "original": {"sha256": "a".repeat(64), "bytes": 40_000},
        "pcm": {"format": "f32-le-interleaved-v1",
            "channel_layout": "interleaved-channel-index-order-v1",
            "rate_hz": RATE, "channels": 1, "full_frames": FRAMES, "full_bytes": FRAMES * 4,
            "interleaved_sha256": "b".repeat(64), "mono_sha256": "c".repeat(64),
            "mono_revision": "mapped-clock-fixture", "source_zero_bits": bits(0.0),
            "provenance": {"synthetic_geometry_only": true}}
    });
    let anchors: Vec<Value> = anchors.iter().enumerate().map(|(index, (source, beat))| json!({
        "id": format!("a-{index}"), "source_seconds_bits": bits(*source),
        "beat_bits": bits(*beat), "raw_beat_index": null, "review": "reviewed", "sample_edit": null
    })).collect();
    let intervals: Vec<Value> = coverage
        .iter()
        .map(|coverage| {
            json!({
                "coverage": coverage, "meter": {
                    "status": if verified_meter { "quarter_note_4_4_verified" } else { "unknown" },
                    "downbeat_label": if verified_meter { Some(0_i64) } else { None },
                    "evidence": "synthetic only"}
            })
        })
        .collect();
    let mut record = json!({"schema_version": 1, "encoding": "source-beatmap-pwa-v1", "record": {
        "source": {"decoder_identity": descriptor_digest(&decoder).unwrap(), "decoder_descriptor": decoder},
        "raw": {"detector": {"detector_id": "manual", "code_revision": "mapped-clock-test-v1",
            "configuration": {}, "model": null, "preprocessing": {}}, "analysis_input": null,
            "timestamp_resolution_seconds_bits": null, "beat_seconds_bits": [],
            "downbeat_seconds_bits": [], "uncertainty": "no source or musical acceptance"},
        "accepted": {"parent_revision": null, "beat_unit": "quarter_note",
            "interpolation_policy": "pwa-source-seconds-quarter-v1",
            "boundary_policy": "right-interior-left-final-gap-unavailable-v1",
            "snap_policy": "nearest-quarter-subdivision-later-tie-v1",
            "anchors": anchors, "intervals": intervals, "lineage": []}
    }});
    record["record"]["raw"]["raw_revision"] =
        json!(descriptor_digest(&record["record"]["raw"]).unwrap());
    record["record"]["accepted"]["map_revision"] =
        json!(descriptor_digest(&record["record"]).unwrap());
    SourceBeatmap::parse(&record.to_string()).unwrap()
}

fn epoch(phase: f64) -> MappedClockEpoch {
    let mut timeline = TransportTimeline::new(RATE);
    assert!(timeline.set_master_period(1.0));
    MappedClockEpoch::new(7, timeline, phase).unwrap()
}

fn geometry(map: SourceBeatmap, quarters: f64, end: usize) -> MappedLoopGeometry {
    MappedLoopGeometry::new(
        map,
        FRAMES,
        RATE,
        FrameRange { start: 0, end },
        quarters,
        false,
    )
    .unwrap()
}

fn coordinate(position: FractionalSourcePosition) -> f64 {
    assert_eq!(position.seek_mode, ExplicitSeekMode::Normal);
    position.frame as f64 + position.fraction
}

#[test]
fn pause_translation_reuses_exact_old_addresses_and_refuses_pre_pause_extrapolation() {
    let geometry = geometry(
        map(&[(0.0, 0.0), (1.2, 1.0)], &["trusted"], false),
        1.0,
        120,
    );
    let original = epoch(-0.373);
    let resumed = original
        .resumed_at(&original.timeline, 31, 987_654_321)
        .unwrap();
    let delay = 987_654_321 - 31;
    assert_eq!(
        geometry.position_at(resumed, delay - 1),
        Err(MappedClockError::NumericalUnavailable)
    );
    assert_eq!(
        geometry.refresh_span(resumed, delay - 1).unwrap_err(),
        MappedClockError::NumericalUnavailable
    );
    for active in [0, 1, 31, 32, 127, 100_003] {
        let before = geometry.position_at(original, active).unwrap();
        let after = geometry.position_at(resumed, active + delay).unwrap();
        assert_eq!(before.frame, after.frame);
        assert_eq!(before.fraction.to_bits(), after.fraction.to_bits());
        let span = geometry.refresh_span(resumed, active + delay).unwrap();
        assert!(span.start_frame() >= delay);
        assert_eq!(span.position_at(active + delay), Some(after));
    }
    assert!(original.resumed_at(&original.timeline, 31, 30).is_none());
    let mut stale = original.timeline;
    assert!(stale.set_master_period_preserving_beat_position_at_frame(0.81, 17));
    assert!(original.resumed_at(&stale, 31, 91).is_none());
    let terminal = resumed
        .resumed_at(&original.timeline, 987_654_321, u64::MAX)
        .unwrap();
    // A terminal address can be projected, but no nonempty exclusive span can
    // start there. SourcePlayback must refuse that unusable continuation.
    assert_eq!(
        geometry.refresh_span(terminal, u64::MAX).unwrap_err(),
        MappedClockError::NumericalUnavailable
    );
}

#[test]
fn paused_master_claim_anchors_at_claim_frame_and_keeps_legacy_projection_exact() {
    let original = epoch(-0.373);
    let paused = original.resumed_at(&original.timeline, 31, 10_003).unwrap();
    let execution = 10_019;
    let mut candidate = original.timeline;
    assert!(candidate.set_master_period_preserving_beat_position_at_frame(0.81, execution));
    let claimed = paused.with_timeline_at(candidate, execution).unwrap();
    assert_eq!(
        claimed.total_at(execution).unwrap().to_bits(),
        paused.total_at(execution).unwrap().to_bits()
    );
    assert_eq!(claimed.total_at(execution - 1), None);
    for frame in [execution, execution + 1, execution + 99] {
        let expected = paused.total_at(execution).unwrap()
            + (candidate.beat_position_at_frame(frame).unwrap()
                - candidate.beat_position_at_frame(execution).unwrap());
        assert_eq!(
            claimed.total_at(frame).unwrap().to_bits(),
            expected.to_bits()
        );
    }
    let again = claimed
        .resumed_at(&candidate, execution + 7, execution + 101)
        .unwrap();
    assert_eq!(again.total_at(execution + 93), None);
    assert_eq!(
        again.total_at(execution + 101).unwrap().to_bits(),
        claimed.total_at(execution + 7).unwrap().to_bits()
    );
    let legacy = original.with_timeline(candidate).unwrap();
    assert!(legacy.matches_exact(original.with_timeline_at(candidate, execution).unwrap()));
}

#[test]
fn fractional_point_three_output_knot_and_many_knots_use_canonical_exclusive_endpoint() {
    let map = map(
        &[
            (0.0, 0.0),
            (0.003, 0.003),
            (0.006, 0.005),
            (0.009, 0.007),
            (0.012, 0.01),
            (1.2, 1.0),
        ],
        &["trusted"; 5],
        false,
    );
    let geometry = geometry(map, 1.0, 120);
    let epoch = epoch(0.0);
    let first = geometry.refresh_span(epoch, 0).unwrap();
    assert_eq!(first.start_frame(), 0);
    assert_eq!(first.end_frame(), 1);
    assert_eq!(first.local_rate(), Some(1.0));
    let canonical = geometry.position_at(epoch, 1).unwrap();
    assert_eq!(first.position_at(1), Some(canonical));
    assert_eq!(coordinate(canonical), 1.2);
    assert_ne!(coordinate(canonical), first.local_rate().unwrap());
    assert_eq!(first.position_at(2), None);
    let next = geometry.refresh_span(epoch, 1).unwrap();
    assert_eq!(next.start_frame(), 1);
    assert_eq!(next.position_at(1), Some(canonical));
}

#[test]
fn exact_thirty_two_quarter_cycle_preserves_unrounded_source_end() {
    let geometry = geometry(
        map(
            &[(0.0, 0.0), (32.004, 32.0), (40.004, 40.0)],
            &["trusted"; 2],
            true,
        ),
        32.0,
        3200,
    );
    let epoch = epoch(0.0);
    assert_eq!(geometry.exact_quarters(), 32.0);
    assert_eq!(geometry.unrounded_source_end(), 32.004);
    assert!((geometry.domain().musical_period.unwrap() - 3200.4).abs() < 1e-10);
    for cycle in [1, 2, 7, 100_000_001_u64] {
        let frame = cycle * 3200;
        assert_eq!(coordinate(geometry.position_at(epoch, frame).unwrap()), 0.0);
        assert_eq!(
            geometry.refresh_span(epoch, frame).unwrap().start_frame(),
            frame
        );
    }
    let span = geometry.refresh_span(epoch, 3199).unwrap();
    assert_eq!(span.end_frame(), 3200);
    assert_eq!(coordinate(span.position_at(3200).unwrap()), 0.0);
}

#[test]
fn irregular_partition_and_midspan_refresh_retain_exact_global_anchors() {
    let geometry = geometry(
        map(
            &[
                (0.0, 0.0),
                (0.407, 0.5),
                (1.251, 1.0),
                (1.7, 1.6),
                (2.4, 2.0),
            ],
            &["trusted"; 4],
            false,
        ),
        2.0,
        240,
    );
    let epoch = epoch(-0.373);
    for start in [0, 19, 91, 199, 200, 201, 10_000_019, 987_654_321_091] {
        let original = geometry.refresh_span(epoch, start).unwrap();
        let again = geometry
            .refresh_span(epoch, original.end_frame() - 1)
            .unwrap();
        assert!(original.matches_exact(again));
        let mut cursor = start;
        for count in [1_u64, 7, 3, 29, 2, 11, 53] {
            let span = geometry.refresh_span(epoch, cursor).unwrap();
            let end = cursor.saturating_add(count).min(span.end_frame());
            for frame in cursor..=end {
                let direct = geometry.position_at(epoch, frame).unwrap();
                assert_eq!(span.position_at(frame), Some(direct), "frame={frame}");
                let beat = geometry.start_beat()
                    + (epoch.timeline.beat_position_at_frame(frame).unwrap()
                        + epoch.phase_quarters)
                        .rem_euclid(geometry.exact_quarters());
                let canonical =
                    geometry.map().canonical_source_at_beat(beat).unwrap() * f64::from(RATE);
                assert_eq!(coordinate(direct).to_bits(), canonical.to_bits());
            }
            cursor = end;
        }
    }
}

#[test]
fn all_whole_interval_gaps_uncertainty_and_invalid_loaded_geometry_refuse() {
    for unsupported in ["gap_unknown_count", "uncertain"] {
        let mapped = map(
            &[(0.0, 0.0), (1.0, 1.0), (2.0, 2.0), (3.0, 3.0)],
            &["trusted", unsupported, "trusted"],
            true,
        );
        assert!(matches!(
            MappedLoopGeometry::new(
                mapped,
                FRAMES,
                RATE,
                FrameRange { start: 0, end: 300 },
                3.0,
                true
            ),
            Err(MappedClockError::UnsupportedCoverage)
        ));
    }
    let mapped = map(&[(0.0, 0.0), (2.0, 2.0)], &["trusted"], false);
    assert!(matches!(
        MappedLoopGeometry::new(
            mapped.clone(),
            FRAMES,
            RATE,
            FrameRange { start: 0, end: 200 },
            2.0,
            true
        ),
        Err(MappedClockError::UnsupportedCoverage)
    ));
    for (rate, end, quarters) in [
        (0, 200, 2.0),
        (RATE, 201, 2.0),
        (RATE, 200, f64::NAN),
        (RATE, 200, 0.0),
        (RATE, 300, 3.0),
    ] {
        assert!(
            MappedLoopGeometry::new(
                mapped.clone(),
                FRAMES,
                rate,
                FrameRange { start: 0, end },
                quarters,
                false
            )
            .is_err()
        );
    }
}

#[test]
fn rate_extrema_reject_even_an_unvisited_segment_without_clamping() {
    let geometry = geometry(
        map(
            &[(0.0, 0.0), (0.01, 0.01), (0.1, 0.02), (1.08, 1.0)],
            &["trusted"; 3],
            false,
        ),
        1.0,
        108,
    );
    let epoch = epoch(0.0);
    assert_eq!(
        geometry.validate_epoch(epoch),
        Err(MappedClockError::UnsupportedRate)
    );
    assert!(matches!(
        geometry.position_at(epoch, 0),
        Err(MappedClockError::UnsupportedRate)
    ));
    assert!(matches!(
        geometry.refresh_span(epoch, 0),
        Err(MappedClockError::UnsupportedRate)
    ));
}

#[test]
fn epoch_identity_ignores_incidental_callback_now_but_preserves_real_anchor() {
    let first = epoch(0.23);
    let mut incidental = first.timeline;
    incidental.advance_by_rendered_frames(997);
    let same = MappedClockEpoch::new(first.marker, incidental, first.phase_quarters).unwrap();
    assert!(first.matches_exact(same));
    assert!(!first.matches_exact(MappedClockEpoch::new(8, incidental, 0.23).unwrap()));
    assert!(!first.matches_exact(MappedClockEpoch::new(7, incidental, 0.24).unwrap()));
    assert!(incidental.anchor_beat_position_at_frame(1.0, 100));
    assert!(!first.matches_exact(MappedClockEpoch::new(7, incidental, 0.23).unwrap()));
    assert!(MappedClockEpoch::new(0, incidental, 0.23).is_none());
    assert!(MappedClockEpoch::new(7, incidental, f64::NAN).is_none());
}

#[test]
fn nonzero_physical_start_and_negative_map_beats_use_relative_loop_phase() {
    let mapped = map(
        &[(0.0, -4.0), (1.0, -3.0), (2.0, -2.0), (3.0, -1.0)],
        &["trusted"; 3],
        false,
    );
    let geometry = MappedLoopGeometry::new(
        mapped,
        FRAMES,
        RATE,
        FrameRange {
            start: 100,
            end: 200,
        },
        1.0,
        false,
    )
    .unwrap();
    let epoch = epoch(0.25);
    assert_eq!(geometry.start_beat(), -3.0);
    assert_eq!(coordinate(geometry.position_at(epoch, 0).unwrap()), 125.0);
    let span = geometry.refresh_span(epoch, 0).unwrap();
    assert_eq!(span.start_frame(), 0);
    assert_eq!(span.end_frame(), 75);
    assert_eq!(coordinate(span.position_at(75).unwrap()), 100.0);
    assert!(span.matches_exact(geometry.refresh_span(epoch, 74).unwrap()));
}

#[test]
fn decimal_cycle_boundaries_follow_remainder_even_next_to_binary64_wrap() {
    let geometry = geometry(
        map(
            &[(0.0, 0.0), (0.03, 0.03), (0.1, 0.1)],
            &["trusted"; 2],
            false,
        ),
        0.1,
        10,
    );
    for phase in [
        0.3_f64.next_down(),
        0.3,
        0.3_f64.next_up(),
        -0.1_f64.next_down(),
    ] {
        let epoch = epoch(phase);
        for frame in 0..200 {
            let span = geometry.refresh_span(epoch, frame).unwrap();
            assert_eq!(
                span.position_at(frame),
                Some(geometry.position_at(epoch, frame).unwrap())
            );
            assert_eq!(
                span.position_at(span.end_frame()),
                Some(geometry.position_at(epoch, span.end_frame()).unwrap())
            );
            assert!(
                span.matches_exact(geometry.refresh_span(epoch, span.end_frame() - 1).unwrap())
            );
        }
    }
}

#[test]
fn bounded_copy_span_carries_no_owned_map_or_dynamic_storage() {
    fn copy<T: Copy>() {}
    copy::<MappedClockEpoch>();
    copy::<MappedSourceSpan>();
    copy::<crate::audio_engine::source_playback::SourcePlayback>();
    assert!(!std::mem::needs_drop::<MappedSourceSpan>());
    assert!(!std::mem::needs_drop::<
        crate::audio_engine::source_playback::SourcePlayback,
    >());
    assert!(std::mem::size_of::<MappedSourceSpan>() <= 240);
    let cohort_bytes = std::mem::size_of::<
        [Option<crate::audio_engine::source_playback::SourcePlayback>;
            crate::audio_engine::constants::MAX_VOICES],
    >();
    assert!(cohort_bytes <= 16 * 1024);
    eprintln!(
        "mapped_clock_sizes: epoch={} span={} playback={} source_feed_span={}",
        std::mem::size_of::<MappedClockEpoch>(),
        std::mem::size_of::<MappedSourceSpan>(),
        std::mem::size_of::<crate::audio_engine::source_playback::SourcePlayback>(),
        std::mem::size_of::<crate::audio_engine::source_feed_history::SourceFeedSpan>()
    );
    let voices = crate::audio_engine::constants::MAX_VOICES;
    let slots = crate::audio_engine::source_feed_history::SOURCE_FEED_HISTORY_MAX_SPANS;
    let slot_bytes =
        std::mem::size_of::<Option<crate::audio_engine::source_feed_history::SourceFeedSpan>>();
    eprintln!(
        "mapped_continuation_sizes: checkpoint={} cohort_stack={} option_history_span={} \
         slots_per_owner={} startup_journals={} startup_prospective_snapshots={} \
         max_control_allocated_action_snapshots=64 startup_span_payload_bytes={} \
         max_simultaneous_span_payload_bytes={} (excludes inline work, Native/PCM, allocator/Arc/ring overhead)",
        std::mem::size_of::<crate::audio_engine::prepared_native_history::SourceReadCheckpoint>(),
        cohort_bytes,
        slot_bytes,
        slots,
        voices,
        voices,
        2 * voices * slots * slot_bytes,
        (2 * voices + 64) * slots * slot_bytes,
    );
}

mod playback_tests {
    include!("mapped_source_playback_tests.rs");
}
