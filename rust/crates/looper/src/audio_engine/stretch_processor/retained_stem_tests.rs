//! Existing scalar stem command handlers on a retained, genuinely offset Native feed.
//! Device-free chronology/PCM evidence; no audible anchor or device acceptance claim.

use super::*;
use crate::audio_engine::prepared_source::{
    capture_prepared_source, enqueue_current_prepared_stems,
};
use crate::audio_engine::productive_source_history::ProductiveSourceHistory;
use crate::audio_engine::stem_cache::source_version_hash;
use crate::messages::{ControlMessage, PreparedStemSet, STEM_COMPONENT_MASK, StemMixMode};

fn install_retained_stems(fixture: &mut Fixture) -> PreparedStemSet {
    let digest = fixture
        .control
        .engine
        .loaded_source_digest(0)
        .unwrap()
        .unwrap();
    let version = format!("samples/retained-synthetic.wav|sha256-v1:{digest}");
    // Capture the real current Native source/timing owner. The production enqueue
    // kernel admits and marks this original permit pending; no test permit/epoch.
    let ticket = capture_prepared_source(&fixture.control.engine, 0, version.clone()).unwrap();
    let sample = &ticket.sample;
    assert!(sample.resident_start() > 0);
    assert!(sample.resident_end() < sample.frame_count());
    let stems = PreparedStemSet {
        complete_set_identity: Arc::new([0x6d; 32]),
        reference_samples: sample.samples.clone(),
        reference_residency: sample.residency.clone(),
        publication: ticket.publication.clone(),
        accepted_timing: ticket.publication.accepted_projection(),
        source_version_hash: source_version_hash(&version),
        sample_rate_hz: ticket.sample_rate_hz,
        channels: sample.channels,
        frame_count: sample.frame_count(),
        available_mask: STEM_COMPONENT_MASK,
        stems: std::array::from_fn(|component| SampleBuffer {
            channels: sample.channels,
            residency: sample.residency.clone(),
            samples: (sample.resident_start()..sample.resident_end())
                .map(|frame| {
                    // Distinct finite components; selecting stems changes actual PCM.
                    (frame as f32 * (0.031 + component as f32 * 0.017)).sin()
                        * (0.04 + component as f32 * 0.025)
                })
                .collect::<Vec<_>>()
                .into(),
        }),
    };
    enqueue_current_prepared_stems(
        &fixture.control.engine,
        &fixture.control.producer,
        &ticket,
        &version,
        stems.clone(),
    )
    .unwrap();
    assert_eq!(ticket.publication_status(), "pending");
    fixture.drain();
    assert_eq!(ticket.publication_status(), "accepted");
    stems
}

fn retained_stem_fixture() -> (
    Fixture,
    SourcePlayback,
    SourcePlayback,
    usize,
    PreparedStemSet,
) {
    let mut installed = None;
    let (fixture, logical, actual, native) = retained_fixture_with_setup(|fixture| {
        installed = Some(install_retained_stems(fixture));
    });
    (fixture, logical, actual, native, installed.unwrap())
}

fn captured_stem_selection(fixture: &Fixture) -> StemRenderSelection {
    let lane = fixture
        .mixer
        .voices
        .iter()
        .position(|voice| voice.is_playing_sample(0))
        .unwrap();
    fixture
        .mixer
        .capture_pitch_voice_snapshot(lane)
        .expect("accepted selection lost its retained logical-to-actual source mapping")
        .plan
        .selection
}

fn queue_stem_mode(fixture: &Fixture, mode: StemMixMode, hash: u64) {
    // The public Native method needs a hardware stream_handle. Use its exact
    // existing scalar command and the real producer/drain/handler on this fixture.
    fixture
        .control
        .producer
        .lock()
        .unwrap()
        .push(ControlMessage::SetStemMixMode {
            id: 0,
            mode,
            source_version_hash: hash,
        })
        .unwrap();
}

fn queue_stem_mask(fixture: &Fixture, mask: u8, hash: u64) {
    fixture
        .control
        .producer
        .lock()
        .unwrap()
        .push(ControlMessage::SetStemEnabledMask {
            id: 0,
            enabled_stem_mask: mask,
            source_version_hash: hash,
        })
        .unwrap();
}

fn queue_stem_pause(fixture: &Fixture, paused: bool) {
    fixture
        .control
        .producer
        .lock()
        .unwrap()
        .push(if paused {
            ControlMessage::PauseSample { id: 0 }
        } else {
            ControlMessage::ResumeSample { id: 0 }
        })
        .unwrap();
}

struct ScalarRetainedCapture {
    logical: SourcePlayback,
    productive: ProductiveSourceHistory,
    native: usize,
    origin: Option<(PitchIntent, bool, u64)>,
    fifo: (Vec<f32>, Vec<f32>),
    generation: u64,
    starts: usize,
    selection: StemRenderSelection,
    transition: StemTransition,
    journal: SourceFeedSnapshot,
}

impl ScalarRetainedCapture {
    fn new(fixture: &Fixture) -> Self {
        let voice = fixture.voice(0);
        let lane = fixture
            .mixer
            .voices
            .iter()
            .position(|voice| voice.is_playing_sample(0))
            .unwrap();
        let snapshot = fixture
            .mixer
            .capture_pitch_voice_snapshot(lane)
            .expect("retained scalar capture needs its actual source mapping");
        let mut journal = SourceFeedSnapshot::new();
        assert!(voice.source_feed_history.capture_into(&mut journal));
        Self {
            logical: voice.source_playback,
            productive: voice.stretch.productive_history().unwrap(),
            native: voice.stretch.native_state_address(),
            origin: voice.stretch.native_origin_tuple(),
            fifo: retained_fifo_samples(fixture),
            generation: voice.generation,
            starts: fixture.starts(0),
            selection: snapshot.plan.selection,
            transition: snapshot.plan.transition,
            journal,
        }
    }

    fn assert_claim_kept_history(&self, fixture: &Fixture, selection: StemRenderSelection) {
        let voice = fixture.voice(0);
        assert!(voice.source_playback.matches_exact(&self.logical));
        let actual = voice.stretch.productive_history().unwrap();
        assert_eq!(actual.next_position, self.productive.next_position);
        assert_eq!(actual.fed_output_frames, self.productive.fed_output_frames);
        assert_eq!(actual.binding, self.productive.binding);
        assert_eq!(actual.domain, self.productive.domain);
        assert_eq!(voice.stretch.native_state_address(), self.native);
        assert_eq!(voice.stretch.native_origin_tuple(), self.origin);
        assert_eq!(retained_fifo_samples(fixture), self.fifo);
        assert_eq!(voice.generation, self.generation);
        assert_eq!(fixture.starts(0), self.starts);
        assert_eq!(captured_stem_selection(fixture), selection);
        let mut journal = SourceFeedSnapshot::new();
        assert!(voice.source_feed_history.capture_into(&mut journal));
        assert_eq!(journal.active_frames(), self.journal.active_frames());
        assert_eq!(journal.active_origin(), self.journal.active_origin());
        assert_eq!(journal.retained_range(), self.journal.retained_range());
        assert_eq!(journal.spans().len(), self.journal.spans().len());
        for (after, before) in journal.spans().zip(self.journal.spans()) {
            assert_eq!(after.active_start, before.active_start);
            assert_eq!(after.frames, before.frames);
            assert!(after.playback.matches_exact(&before.playback));
            assert!(after.plan.matches_exact(before.plan));
            assert_eq!(after.context, before.context);
        }
    }

    fn assert_refused(&self, fixture: &Fixture) {
        self.assert_claim_kept_history(fixture, self.selection);
        let lane = fixture
            .mixer
            .voices
            .iter()
            .position(|voice| voice.is_playing_sample(0))
            .unwrap();
        let snapshot = fixture.mixer.capture_pitch_voice_snapshot(lane).unwrap();
        assert!(snapshot.plan.transition.matches_exact(self.transition));
        let mut journal = SourceFeedSnapshot::new();
        assert!(
            fixture
                .voice(0)
                .source_feed_history
                .capture_into(&mut journal)
        );
        assert_eq!(journal.captured_epoch(), self.journal.captured_epoch());
    }
}

fn render_scalar_pair(fixture: &mut Fixture, reference: &mut Fixture, total: usize) -> Vec<f32> {
    let native = fixture.voice(0).stretch.native_state_address();
    let reference_native = reference.voice(0).stretch.native_state_address();
    let generation = fixture.voice(0).generation;
    let reference_generation = reference.voice(0).generation;
    let starts = fixture.starts(0);
    let reference_starts = reference.starts(0);
    assert_ne!(native, reference_native);
    let mut output = Vec::with_capacity(total);
    let mut elapsed = 0;
    let mut partition = 0;
    let partitions = [1, 17, 31, 511, 257, 1, 512, 73];
    while elapsed < total {
        let frames = partitions[partition % partitions.len()].min(total - elapsed);
        let fed = fixture
            .voice(0)
            .stretch
            .productive_history()
            .unwrap()
            .fed_output_frames;
        let active_frames = if fixture.voice(0).paused {
            0
        } else {
            frames as u64
        };
        let pcm = fixture.render(frames);
        let mut expected = Vec::with_capacity(frames);
        let mut remaining = frames;
        while remaining != 0 {
            let count = remaining.min(113);
            expected.extend(reference.render(count));
            remaining -= count;
        }
        assert!(pcm.iter().all(|value| value.is_finite()));
        assert_eq!(
            pcm, expected,
            "unequal partitions changed retained scalar PCM"
        );
        assert_eq!(fixture.voice(0).stretch.native_state_address(), native);
        assert_eq!(
            reference.voice(0).stretch.native_state_address(),
            reference_native
        );
        assert_eq!(fixture.voice(0).generation, generation);
        assert_eq!(reference.voice(0).generation, reference_generation);
        assert_eq!(fixture.starts(0), starts);
        assert_eq!(reference.starts(0), reference_starts);
        assert_eq!(
            fixture
                .voice(0)
                .stretch
                .productive_history()
                .unwrap()
                .fed_output_frames,
            fed + active_frames,
            "scalar selection lost successful actual Native input frames"
        );
        assert_eq!(
            retained_fifo_samples(fixture),
            retained_fifo_samples(reference)
        );
        output.extend(pcm);
        elapsed += frames;
        partition += 1;
    }
    output
}

fn assert_last_stem_read(fixture: &Fixture, stems: &PreparedStemSet, mask: Option<u8>) {
    let mut journal = SourceFeedSnapshot::new();
    assert!(
        fixture
            .voice(0)
            .source_feed_history
            .capture_into(&mut journal)
    );
    let span = journal.spans().last().unwrap();
    let selection = mask.map_or_else(StemRenderSelection::full_mix, |mask| {
        StemRenderSelection::for_stems(stems, mask)
    });
    assert_eq!(span.plan.selection, selection);
    if mask.is_some() {
        assert_eq!(
            span.context.current_stem_owners[0],
            Arc::as_ptr(&stems.complete_set_identity) as usize
        );
    } else {
        assert_eq!(span.context.current_stem_owners, [0; 6]);
    }
    assert_eq!(
        span.playback.position_at(span.frames),
        fixture
            .voice(0)
            .stretch
            .productive_history()
            .unwrap()
            .next_position
    );
}

#[test]
fn real_scalar_stem_selection_keeps_retained_native_fifos_and_actual_feed() {
    let (mut fixture, logical, actual, native, stems) = retained_stem_fixture();
    let (mut reference, reference_logical, reference_actual, reference_native, reference_stems) =
        retained_stem_fixture();
    assert_ne!(native, reference_native);
    let generation = fixture.voice(0).generation;
    let starts = fixture.starts(0);
    let mut elapsed = 0;
    let mut before = Vec::new();
    for frames in [17, 31, 1, 511] {
        before.extend(fixture.render(frames));
        elapsed += frames;
        assert_retained_coordinates(&fixture, &logical, &actual, elapsed);
    }
    assert_nonzero_pcm(&before);
    assert_eq!(before, reference.render(elapsed));
    assert_retained_coordinates(&reference, &reference_logical, &reference_actual, elapsed);

    let capture = fixture.transport.output_frame();
    let logical_at_capture = fixture.voice(0).source_playback;
    let actual_at_capture = fixture.voice(0).stretch.productive_history().unwrap();
    let origin = fixture.voice(0).stretch.native_origin_tuple();
    let fifo = retained_fifo_samples(&fixture);
    assert_ne!(
        logical_at_capture.position(),
        actual_at_capture.next_position
    );
    assert_eq!(fifo, retained_fifo_samples(&reference));
    queue_stem_mode(&fixture, StemMixMode::AllStems, stems.source_version_hash);
    queue_stem_mode(
        &reference,
        StemMixMode::AllStems,
        reference_stems.source_version_hash,
    );
    // Queueing has no callback effect. The genuine installed owner stays present.
    assert_eq!(
        captured_stem_selection(&fixture),
        StemRenderSelection::full_mix()
    );
    assert_eq!(fixture.transport.output_frame(), capture);
    assert_eq!(retained_fifo_samples(&fixture), fifo);
    fixture.drain();
    reference.drain();
    assert_eq!(
        captured_stem_selection(&fixture),
        StemRenderSelection::for_stems(&stems, STEM_COMPONENT_MASK)
    );
    assert_eq!(fixture.transport.output_frame(), capture);
    assert_eq!(fixture.voice(0).stretch.native_state_address(), native);
    assert_eq!(fixture.voice(0).stretch.native_origin_tuple(), origin);
    assert_eq!(retained_fifo_samples(&fixture), fifo);
    assert!(
        fixture
            .voice(0)
            .source_playback
            .matches_exact(&logical_at_capture)
    );
    assert_eq!(
        fixture
            .voice(0)
            .stretch
            .productive_history()
            .unwrap()
            .next_position,
        actual_at_capture.next_position
    );
    assert_eq!(fixture.voice(0).generation, generation);
    assert_eq!(fixture.starts(0), starts);

    // This is the first meaningful failure on the old path: selection is accepted,
    // but the stale retained plan rejects the next actual read and loses Native feed.
    let mut continued = Vec::new();
    for frames in [1, 17, 31, 511, 257, 1, 512, 73] {
        let pcm = fixture.render(frames);
        continued.extend(pcm);
        elapsed += frames;
        assert_retained_coordinates(&fixture, &logical, &actual, elapsed);
        assert_eq!(fixture.voice(0).stretch.native_state_address(), native);
        assert_eq!(fixture.voice(0).stretch.native_origin_tuple(), origin);
        assert_eq!(fixture.voice(0).generation, generation);
        assert_eq!(fixture.starts(0), starts);
        let mut journal = SourceFeedSnapshot::new();
        assert!(
            fixture
                .voice(0)
                .source_feed_history
                .capture_into(&mut journal)
        );
        let span = journal.spans().last().unwrap();
        assert_eq!(
            span.plan.selection,
            StemRenderSelection::for_stems(&stems, STEM_COMPONENT_MASK)
        );
        assert_eq!(
            span.context.current_stem_owners[0],
            Arc::as_ptr(&stems.complete_set_identity) as usize
        );
    }
    assert_nonzero_pcm(&continued);
    let expected = reference.render_to(fixture.transport.output_frame());
    assert_nonzero_pcm(&expected);
    assert_eq!(
        continued, expected,
        "unequal partitions changed retained stem PCM"
    );
    assert_retained_coordinates(&reference, &reference_logical, &reference_actual, elapsed);
    assert_eq!(
        reference.voice(0).stretch.native_state_address(),
        reference_native
    );
}

#[test]
fn scalar_empty_mask_then_restore_and_full_mix_keep_one_retained_native() {
    let (mut fixture, logical, actual, native, stems) = retained_stem_fixture();
    let (mut reference, reference_logical, reference_actual, reference_native, reference_stems) =
        retained_stem_fixture();
    let generation = fixture.voice(0).generation;
    let starts = fixture.starts(0);
    let mut elapsed = 560;
    assert_nonzero_pcm(&render_scalar_pair(&mut fixture, &mut reference, elapsed));

    // These are scalar changes on the same genuinely published owner. Each claim
    // preserves the existing pending input/output samples, even for an empty mask.
    for (step, selection, frames) in [
        (0, Some(STEM_COMPONENT_MASK), 1449),
        (1, Some(0), 32_768),
        (2, Some(0b1010), 4096),
        (3, None, 1449),
    ] {
        let before = ScalarRetainedCapture::new(&fixture);
        let reference_before = ScalarRetainedCapture::new(&reference);
        let capture = fixture.transport.output_frame();
        for (current, set) in [(&fixture, &stems), (&reference, &reference_stems)] {
            match step {
                0 => queue_stem_mode(current, StemMixMode::AllStems, set.source_version_hash),
                1 | 2 => queue_stem_mask(current, selection.unwrap(), set.source_version_hash),
                3 => queue_stem_mode(current, StemMixMode::FullMix, 0),
                _ => unreachable!(),
            }
        }
        before.assert_refused(&fixture);
        reference_before.assert_refused(&reference);
        fixture.drain();
        reference.drain();
        assert_eq!(fixture.transport.output_frame(), capture);
        assert_eq!(reference.transport.output_frame(), capture);
        let expected_selection = selection.map_or_else(StemRenderSelection::full_mix, |mask| {
            StemRenderSelection::for_stems(&stems, mask)
        });
        let reference_selection = selection.map_or_else(StemRenderSelection::full_mix, |mask| {
            StemRenderSelection::for_stems(&reference_stems, mask)
        });
        before.assert_claim_kept_history(&fixture, expected_selection);
        reference_before.assert_claim_kept_history(&reference, reference_selection);
        let pcm = render_scalar_pair(&mut fixture, &mut reference, frames);
        elapsed += frames;
        assert_retained_coordinates(&fixture, &logical, &actual, elapsed);
        assert_retained_coordinates(&reference, &reference_logical, &reference_actual, elapsed);
        assert_last_stem_read(&fixture, &stems, selection);
        assert_last_stem_read(&reference, &reference_stems, selection);
        assert_eq!(fixture.voice(0).stretch.native_state_address(), native);
        assert_eq!(
            reference.voice(0).stretch.native_state_address(),
            reference_native
        );
        assert_eq!(fixture.voice(0).generation, generation);
        assert_eq!(fixture.starts(0), starts);
        if selection == Some(0) {
            // A long bounded drain removes preselection FIFO/Native content. Empty
            // selection still produces successful actual reads and advances both
            // clocks; it must not be confused with a refused or missing source.
            assert!(pcm[pcm.len() - 2048..].iter().all(|value| *value == 0.0));
            assert!(
                fixture.voice(0).stretch.varispeed_buffers()[0]
                    .iter()
                    .all(|value| *value == 0.0)
            );
        } else {
            assert_nonzero_pcm(&pcm);
        }
    }
}

#[test]
fn scalar_wrong_hash_mode_and_mask_refuse_without_mutating_retained_history() {
    let (mut fixture, logical, actual, native, stems) = retained_stem_fixture();
    let (mut reference, reference_logical, reference_actual, reference_native, reference_stems) =
        retained_stem_fixture();
    let wrong_hash = if stems.source_version_hash == u64::MAX {
        stems.source_version_hash - 1
    } else {
        stems.source_version_hash + 1
    };
    assert_ne!(wrong_hash, 0);
    assert_ne!(wrong_hash, stems.source_version_hash);
    let mut elapsed = 560;
    assert_nonzero_pcm(&render_scalar_pair(&mut fixture, &mut reference, elapsed));

    let capture = fixture.transport.output_frame();
    let before = ScalarRetainedCapture::new(&fixture);
    queue_stem_mode(&fixture, StemMixMode::AllStems, wrong_hash);
    fixture.drain();
    assert_eq!(fixture.transport.output_frame(), capture);
    before.assert_refused(&fixture);
    assert_nonzero_pcm(&render_scalar_pair(&mut fixture, &mut reference, 1449));
    elapsed += 1449;
    assert_last_stem_read(&fixture, &stems, None);
    assert_retained_coordinates(&fixture, &logical, &actual, elapsed);

    queue_stem_mode(&fixture, StemMixMode::AllStems, stems.source_version_hash);
    queue_stem_mode(
        &reference,
        StemMixMode::AllStems,
        reference_stems.source_version_hash,
    );
    fixture.drain();
    reference.drain();
    assert_nonzero_pcm(&render_scalar_pair(&mut fixture, &mut reference, 1449));
    elapsed += 1449;
    assert_last_stem_read(&fixture, &stems, Some(STEM_COMPONENT_MASK));

    let capture = fixture.transport.output_frame();
    let before = ScalarRetainedCapture::new(&fixture);
    queue_stem_mask(&fixture, 0, wrong_hash);
    fixture.drain();
    assert_eq!(fixture.transport.output_frame(), capture);
    before.assert_refused(&fixture);
    assert_nonzero_pcm(&render_scalar_pair(&mut fixture, &mut reference, 1449));
    elapsed += 1449;
    assert_last_stem_read(&fixture, &stems, Some(STEM_COMPONENT_MASK));
    assert_retained_coordinates(&fixture, &logical, &actual, elapsed);
    assert_retained_coordinates(&reference, &reference_logical, &reference_actual, elapsed);
    assert_eq!(fixture.voice(0).stretch.native_state_address(), native);
    assert_eq!(
        reference.voice(0).stretch.native_state_address(),
        reference_native
    );
}

#[test]
fn scalar_selection_refuses_while_finite_retained_voice_paused_and_resumes_same_feed() {
    let (mut fixture, logical, actual, native, stems) = retained_stem_fixture();
    let (mut reference, reference_logical, reference_actual, reference_native, reference_stems) =
        retained_stem_fixture();
    let mut elapsed = 560;
    assert_nonzero_pcm(&render_scalar_pair(&mut fixture, &mut reference, elapsed));
    queue_stem_mode(&fixture, StemMixMode::AllStems, stems.source_version_hash);
    queue_stem_mode(
        &reference,
        StemMixMode::AllStems,
        reference_stems.source_version_hash,
    );
    fixture.drain();
    reference.drain();
    assert_nonzero_pcm(&render_scalar_pair(&mut fixture, &mut reference, 1449));
    elapsed += 1449;
    queue_stem_pause(&fixture, true);
    queue_stem_pause(&reference, true);
    fixture.drain();
    reference.drain();
    assert!(fixture.voice(0).paused);
    assert!(reference.voice(0).paused);

    let before = ScalarRetainedCapture::new(&fixture);
    let reference_before = ScalarRetainedCapture::new(&reference);
    let pause_frame = fixture.transport.output_frame();
    queue_stem_mask(&fixture, 0, stems.source_version_hash);
    fixture.drain();
    before.assert_refused(&fixture);
    queue_stem_mode(&fixture, StemMixMode::FullMix, 0);
    fixture.drain();
    before.assert_refused(&fixture);
    assert_eq!(fixture.transport.output_frame(), pause_frame);
    let silence = render_scalar_pair(&mut fixture, &mut reference, 997);
    assert!(silence.iter().all(|value| *value == 0.0));
    assert_eq!(fixture.transport.output_frame(), pause_frame + 997);
    before.assert_refused(&fixture);
    reference_before.assert_refused(&reference);
    assert_retained_coordinates(&fixture, &logical, &actual, elapsed);

    queue_stem_pause(&fixture, false);
    queue_stem_pause(&reference, false);
    fixture.drain();
    reference.drain();
    assert!(!fixture.voice(0).paused);
    assert!(!reference.voice(0).paused);
    before.assert_claim_kept_history(&fixture, before.selection);
    reference_before.assert_claim_kept_history(&reference, reference_before.selection);
    assert_nonzero_pcm(&render_scalar_pair(&mut fixture, &mut reference, 1449));
    elapsed += 1449;
    assert_retained_coordinates(&fixture, &logical, &actual, elapsed);
    assert_retained_coordinates(&reference, &reference_logical, &reference_actual, elapsed);
    assert_last_stem_read(&fixture, &stems, Some(STEM_COMPONENT_MASK));
    assert_eq!(fixture.voice(0).stretch.native_state_address(), native);
    assert_eq!(
        reference.voice(0).stretch.native_state_address(),
        reference_native
    );
}
