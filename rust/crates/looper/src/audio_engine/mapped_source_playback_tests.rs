use super::*;
use crate::audio_engine::productive_source_history::ProductiveSourceBinding;
use crate::audio_engine::rubberband_backend::RubberBandError;
use crate::audio_engine::source_feed_history::{
    SourceFeedContext, SourceFeedHistory, SourceFeedSnapshot,
};
use crate::audio_engine::source_playback::SourcePlayback;
use crate::audio_engine::source_reader::{
    SourceReadPlan, StemReadSets, StemRenderSelection, StemTransition,
};
use crate::messages::SampleBuffer;

fn variable_geometry() -> MappedLoopGeometry {
    geometry(
        map(
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
        ),
        1.0,
        120,
    )
}

#[test]
fn repeated_fractional_pause_projection_matches_uninterrupted_active_address_bits() {
    let geometry = variable_geometry();
    let original_epoch = epoch(0.173);
    let mut playback = SourcePlayback::mapped(&geometry, original_epoch, 0).unwrap();
    let mut shift = 0;
    for active in 0..307 {
        assert_eq!(
            playback.position(),
            geometry.position_at(original_epoch, active).unwrap()
        );
        if [1, 7, 31, 102, 219].contains(&active) {
            let saved = playback.position();
            let duration = 93 + active % 17;
            shift += duration;
            playback
                .resume_mapped_at(&geometry, &timeline(), active + shift)
                .unwrap();
            assert_eq!(playback.position().frame, saved.frame);
            assert_eq!(
                playback.position().fraction.to_bits(),
                saved.fraction.to_bits()
            );
            assert_eq!(playback.mapped_output_frame(), Some(active + shift));
        }
        playback.refresh_mapped_span(&geometry).unwrap();
        assert!(playback.advance(1));
    }
    let before = playback;
    assert!(
        playback
            .resume_mapped_at(&geometry, &timeline(), 1)
            .is_err()
    );
    assert!(playback.matches_exact(&before));
    let mut stale = timeline();
    assert!(stale.set_master_period(0.81));
    assert!(playback.resume_mapped_at(&geometry, &stale, 1000).is_err());
    assert!(playback.matches_exact(&before));
    assert!(
        playback
            .resume_mapped_at(&geometry, &timeline(), u64::MAX)
            .is_err()
    );
    assert!(playback.matches_exact(&before));
}

#[test]
fn copied_playback_refuses_span_overrun_and_keeps_canonical_endpoint() {
    let geometry = variable_geometry();
    let epoch = epoch(0.0);
    let mut playback = SourcePlayback::mapped(&geometry, epoch, 0).unwrap();
    assert!(playback.is_mapped());
    assert_eq!(playback.mapped_output_frame(), Some(0));
    assert!(playback.mapped_epoch().unwrap().matches_exact(epoch));
    assert_eq!(playback.chunk(999), (1, 1.0));
    assert!(playback.can_read_frames(1));
    assert!(!playback.can_read_frames(2));
    assert_eq!(playback.position_at(2), None);
    let before = playback;
    assert!(!playback.try_advance(2));
    assert!(playback.matches_exact(&before));
    assert!(playback.try_advance(1));
    assert_eq!(playback.position(), geometry.position_at(epoch, 1).unwrap());
    assert_eq!(playback.position_at(0), Some(playback.position()));
    assert_eq!(playback.chunk(999).0, 0);
    assert_eq!(playback.position_at(1), None);
    assert_eq!(playback.mapped_output_frame(), Some(1));
    let old_endpoint = playback;
    playback.refresh_mapped_span(&geometry).unwrap();
    assert_eq!(playback.position(), old_endpoint.position());
    assert!(!playback.matches_exact(&old_endpoint));
    assert!(playback.matches_exact(&SourcePlayback::mapped(&geometry, epoch, 1).unwrap()));
    assert!(playback.chunk(999).0 > 0);
}

#[test]
fn callback_cuts_share_descriptor_and_copy_cursor_is_independent() {
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
    let original = SourcePlayback::mapped(&geometry, epoch, 123_456_789).unwrap();
    let mut one_frame = original;
    let mut partitioned = original;
    let mut remaining = partitioned.chunk(999).0;
    let total = remaining;
    while remaining > 0 {
        let count = remaining.min(1 + remaining % 7);
        for offset in 0..count {
            assert_eq!(partitioned.position_at(offset), Some(one_frame.position()));
            assert!(one_frame.advance(1));
        }
        assert!(partitioned.advance(count));
        assert!(partitioned.matches_exact(&one_frame));
        remaining -= count;
    }
    assert!(total > 0);
    assert_eq!(original.mapped_output_frame(), Some(123_456_789));
    assert_ne!(
        original.mapped_output_frame(),
        partitioned.mapped_output_frame()
    );
    assert!(partitioned.matches_rate_target(&original));
}

#[test]
fn distant_projection_requires_borrowed_geometry_and_is_direct_canonical() {
    let geometry = variable_geometry();
    let epoch = epoch(0.17);
    let mut projected = SourcePlayback::mapped(&geometry, epoch, 0).unwrap();
    for target in [32_001, 987_654_321_091, 7, 3_200_000_000_019] {
        projected.project_mapped_to(&geometry, target).unwrap();
        assert_eq!(projected.mapped_output_frame(), Some(target));
        assert_eq!(
            projected.position(),
            geometry.position_at(epoch, target).unwrap()
        );
        assert!(
            projected.matches_exact(&SourcePlayback::mapped(&geometry, epoch, target).unwrap())
        );
    }
    let before = projected;
    let incompatible = super::geometry(
        map(&[(0.0, 0.0), (2.0, 2.0)], &["trusted"], false),
        2.0,
        200,
    );
    assert_eq!(
        projected.project_mapped_to(&incompatible, 100),
        Err(MappedClockError::InvalidGeometry)
    );
    assert!(projected.matches_exact(&before));
}

#[test]
fn epoch_replacement_is_atomic_and_preserves_exact_due_frame_position() {
    let geometry = geometry(
        map(&[(0.0, 0.0), (2.0, 2.0)], &["trusted"], false),
        2.0,
        200,
    );
    let epoch = epoch(0.0);
    let mut playback = SourcePlayback::mapped(&geometry, epoch, 17).unwrap();
    let before = playback;
    let changed_phase = MappedClockEpoch::new(8, timeline(), 0.1).unwrap();
    assert_eq!(
        playback.replace_mapped_epoch(&geometry, changed_phase),
        Err(MappedClockError::InvalidEpoch)
    );
    assert!(playback.matches_exact(&before));
    let mut next_timeline = timeline();
    assert!(next_timeline.set_master_period_preserving_beat_position_at_frame(0.75, 17));
    let new_epoch = MappedClockEpoch::new(8, next_timeline, 0.0).unwrap();
    playback.replace_mapped_epoch(&geometry, new_epoch).unwrap();
    assert_eq!(playback.mapped_output_frame(), Some(17));
    assert_eq!(playback.position(), before.position());
    assert!(!playback.matches_exact(&before));
    assert!(playback.matches_exact(&SourcePlayback::mapped(&geometry, new_epoch, 17).unwrap()));
    assert_ne!(playback.tempo_ratio(), before.tempo_ratio());
}

#[test]
fn scalar_mutators_and_invented_history_cannot_remove_active_map() {
    let geometry = variable_geometry();
    let mut playback = SourcePlayback::mapped(&geometry, epoch(0.0), 9).unwrap();
    let before = playback;
    assert!(!playback.set_target(0.9));
    assert!(!playback.seek(0, ExplicitSeekMode::Normal));
    assert!(!playback.configure_domain(SourceLoopDomain::physical(
        FRAMES,
        FrameRange { start: 0, end: 120 }
    )));
    assert!(playback.configure_domain(geometry.domain()));
    playback.clear_explicit_seek();
    assert!(playback.matches_exact(&before));
    assert!(playback.history_before_constant(1).is_none());
    assert!(playback.shifted_normal_history(1.0).is_none());
    let mut scalar = SourcePlayback::new(
        playback.position().frame,
        ExplicitSeekMode::Normal,
        playback.tempo_ratio(),
    );
    assert!(scalar.configure_domain(geometry.domain()));
    assert!(!playback.matches_exact(&scalar));
    assert!(!playback.matches_rate_target(&scalar));
    assert_eq!(
        scalar.project_mapped_to(&geometry, 9),
        Err(MappedClockError::InvalidEpoch)
    );
    assert!(scalar.advance(1));
}

fn linear_pcm() -> SampleBuffer {
    SampleBuffer {
        samples: (0..FRAMES)
            .flat_map(|frame| [frame as f32, 100.0 + 2.0 * frame as f32])
            .collect::<Vec<_>>()
            .into(),
        channels: 2,
        residency: None,
    }
}

fn actual_read_plan(geometry: &MappedLoopGeometry, playback: SourcePlayback) -> SourceReadPlan {
    let domain = geometry.domain();
    let position = playback.position();
    SourceReadPlan {
        channels: 2,
        sample_frames: domain.sample_frames,
        frame_pos: position.frame,
        loop_region: domain.region,
        loop_period: domain.musical_period,
        seek_mode: position.seek_mode,
        selection: StemRenderSelection::full_mix(),
        transition: StemTransition::default(),
    }
}

#[test]
fn actual_fractional_reader_follows_piecewise_map_through_knot_and_loop_wrap() {
    let geometry = variable_geometry();
    let sample = linear_pcm();
    let epoch = epoch(0.0);
    let render = |partitions: &[usize]| {
        let mut playback = SourcePlayback::mapped(&geometry, epoch, 0).unwrap();
        let mut observed = [Vec::new(), Vec::new()];
        let mut rendered = 0;
        let mut index = 0;
        let mut refreshed_at = Vec::new();
        while rendered < 210 {
            let requested = partitions[index % partitions.len()].min(210 - rendered);
            let (frames, _) = playback.chunk(requested);
            if frames == 0 {
                refreshed_at.push(playback.mapped_output_frame().unwrap());
                playback.refresh_mapped_span(&geometry).unwrap();
                continue;
            }
            let plan = actual_read_plan(&geometry, playback);
            let mut buffers = vec![vec![-999.0; frames]; 2];
            plan.fill_fractional_buffers_checked_with_readers(
                &sample,
                StemReadSets::default(),
                &playback,
                &mut buffers,
                frames,
            )
            .unwrap();
            for (destination, buffer) in observed.iter_mut().zip(buffers) {
                destination.extend(buffer);
            }
            assert!(playback.advance(frames));
            rendered += frames;
            index += 1;
        }
        assert!(refreshed_at.contains(&1)); // The 0.3-frame knot and the other skipped knots.
        assert!(refreshed_at.contains(&100));
        assert!(refreshed_at.contains(&101));
        for frame in 0..210 {
            // Independent integer-output landmarks for this declared fixture:
            // source n=0 is0; all n=1..99 are exactly1.2*n before each100-frame wrap.
            let expected = (frame % 100) as f32 * 1.2;
            assert!(
                (observed[0][frame] - expected).abs() < 0.000_1,
                "left frame={frame}"
            );
            assert!(
                (observed[1][frame] - (100.0 + 2.0 * expected)).abs() < 0.000_1,
                "right frame={frame}"
            );
        }
        observed
    };
    let reference = render(&[1]);
    assert_eq!(render(&[7, 31, 2, 19]), reference);
}

#[test]
fn actual_fractional_reader_refuses_overrun_before_touching_output() {
    let geometry = variable_geometry();
    let sample = linear_pcm();
    let playback = SourcePlayback::mapped(&geometry, epoch(0.0), 0).unwrap();
    let before = playback;
    let plan = actual_read_plan(&geometry, playback);
    for checked in [false, true] {
        let mut buffers = vec![vec![-777.25; 8]; 2];
        let original_buffers = buffers.clone();
        let result = if checked {
            plan.fill_fractional_buffers_checked(&sample, None, &playback, &mut buffers, 2)
        } else {
            plan.fill_fractional_buffers(&sample, None, &playback, &mut buffers, 2)
        };
        assert!(matches!(
            result,
            Err(RubberBandError::UnavailableSourceHistory)
        ));
        assert_eq!(buffers, original_buffers);
        assert!(playback.matches_exact(&before));
    }
}

#[test]
fn actual_read_journal_fences_impossible_mapped_span_and_retains_only_real_suffix() {
    let geometry = variable_geometry();
    let sample = linear_pcm();
    let mut playback = SourcePlayback::mapped(&geometry, epoch(0.0), 0).unwrap();
    let plan = actual_read_plan(&geometry, playback);
    let context = SourceFeedContext::new(
        ProductiveSourceBinding::new(&sample, RATE, None),
        StemReadSets::default(),
    );
    let mut output = vec![vec![0.0; 1]; 2];
    plan.fill_fractional_buffers_checked(&sample, None, &playback, &mut output, 1)
        .unwrap();
    let mut history = SourceFeedHistory::new();
    history.record_success(playback, plan, context, 1);
    let mut genuine_prefix = SourceFeedSnapshot::new();
    assert!(history.capture_into(&mut genuine_prefix));
    assert_eq!(genuine_prefix.retained_range(), Some(0..1));
    assert!(genuine_prefix.is_current());

    history.record_success(playback, plan, context, 2); // This span cannot have been read.
    assert!(!genuine_prefix.is_current());
    let mut after_refusal = SourceFeedSnapshot::new();
    assert!(history.capture_into(&mut after_refusal));
    assert_eq!(after_refusal.active_frames(), 3);
    assert_eq!(after_refusal.retained_range(), None);
    assert_eq!(after_refusal.spans().len(), 0);

    assert!(playback.advance(1));
    playback.refresh_mapped_span(&geometry).unwrap();
    let next_plan = actual_read_plan(&geometry, playback);
    next_plan
        .fill_fractional_buffers_checked(&sample, None, &playback, &mut output, 1)
        .unwrap();
    history.record_success(playback, next_plan, context, 1);
    let mut genuine_suffix = SourceFeedSnapshot::new();
    assert!(history.capture_into(&mut genuine_suffix));
    assert_eq!(genuine_suffix.retained_range(), Some(3..4));
    let span = genuine_suffix.spans().next().unwrap();
    assert_eq!(span.playback.mapped_output_frame(), Some(1));
    assert_eq!(
        span.playback.position(),
        geometry.position_at(epoch(0.0), 1).unwrap()
    );
}
