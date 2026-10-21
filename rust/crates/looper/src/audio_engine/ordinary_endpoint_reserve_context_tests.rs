//! A genuine source-history owner cannot masquerade as a fresh cached birth.
use super::*;
use crate::audio_engine::constant_timing::CurrentTimingAcknowledgements;
use crate::audio_engine::input_runtime_binding::InputRuntimeOwnership;
use crate::audio_engine::pitch_intent::PitchIntent;
use crate::audio_engine::source_reader::{
    ExplicitSeekMode, FrameRange, StemRenderSelection, StemTransition,
};
use crate::messages::SampleBuffer;

#[test]
fn physical_birth_cache_rejects_source_history_without_consuming_either_owner_and_reuses_native() {
    let sample = SampleBuffer {
        channels: 1,
        samples: vec![0.25; 8192].into(),
        residency: None,
    }
    .with_complete_source(48_000);
    let ownership = Arc::new(InputRuntimeOwnership::tracked());
    ownership.publish_test_source(0, &sample, 48_000, 3);
    let acknowledgements = Arc::new(CurrentTimingAcknowledgements::default());
    let epoch = Arc::new(AtomicU64::new(1));
    let permit = NativeHistoryContext {
        id: 0,
        ownership: &ownership,
        acknowledgements: &acknowledgements,
        preparation_epoch: &epoch,
    }
    .capture(&sample, 48_000, None)
    .unwrap();
    let region = FrameRange {
        start: 1024,
        end: 7168,
    };
    let mut playback = SourcePlayback::new(region.start, ExplicitSeekMode::Normal, 1.0);
    assert!(playback.configure(8192, region));
    let request = |request_id| NativeHistoryRequest {
        pitch_intent: PitchIntent::new(0, 7).unwrap(),
        key_lock: true,
        sample: sample.clone(),
        stems: None,
        permit: permit.clone(),
        coverage: None,
        binding: ProductiveSourceBinding::new(&sample, 48_000, None),
        playback,
        plan: SourceReadPlan {
            channels: 1,
            sample_frames: 8192,
            frame_pos: region.start,
            loop_region: region,
            loop_period: None,
            seek_mode: ExplicitSeekMode::Normal,
            selection: StemRenderSelection::full_mix(),
            transition: StemTransition::default(),
        },
        actual_feed: None,
        mapped_input: None,
        target_output_frame: 24_000,
        request_id,
        epoch: 1,
    };
    let mut reserve = EndpointReserveWork::new();
    reserve.request = Some(request(31));
    reserve.prepare();
    assert!(reserve.ready);
    let prepared = reserve.candidate.as_mut().unwrap();
    let address = prepared.rubberband.as_ref().unwrap().state_address();
    assert_ne!(address, 0);
    assert!(prepared.has_physical_birth_context());
    let original = prepared.source.take().unwrap();
    prepared.prepare_source_at_endpoint(original).unwrap();
    assert!(!prepared.has_physical_birth_context());
    assert_eq!(
        prepared.rubberband.as_ref().unwrap().state_address(),
        address
    );

    let mut action_request = Some(request(71));
    let mut action_candidate = None;
    assert!(!reserve.transfer_to(&mut action_request, &mut action_candidate, (48_000, 1)));
    assert!(reserve.ready && reserve.attempted);
    assert!(reserve.request.is_none() && reserve.retired_request.is_none());
    assert!(action_candidate.is_none());
    assert_eq!(action_request.as_ref().unwrap().request_id, 71);
    let retained = reserve.candidate.as_mut().unwrap();
    assert_eq!(
        retained.rubberband.as_ref().unwrap().state_address(),
        address
    );
    assert_eq!(retained.source.as_ref().unwrap().request_id, 31);
    let original = retained.source.take().unwrap();
    retained.prepare_source_at_physical_birth(original).unwrap();
    assert!(retained.has_physical_birth_context());
    assert_eq!(
        retained.rubberband.as_ref().unwrap().state_address(),
        address
    );

    assert!(reserve.transfer_to(&mut action_request, &mut action_candidate, (48_000, 1)));
    assert!(action_request.is_none() && reserve.candidate.is_none());
    assert!(!reserve.ready && !reserve.attempted);
    assert_eq!(reserve.retired_request.as_ref().unwrap().request_id, 31);
    let adopted = action_candidate.unwrap();
    assert!(adopted.has_physical_birth_context());
    assert_eq!(
        adopted.rubberband.as_ref().unwrap().state_address(),
        address
    );
    assert_eq!(adopted.source.as_ref().unwrap().request_id, 71);
}

fn fresh_fixture() -> (
    crate::audio_engine::pitch_runtime_tests::Fixture,
    SampleBuffer,
) {
    let mut fixture = crate::audio_engine::pitch_runtime_tests::Fixture::new(true);
    let sample = SampleBuffer {
        channels: 1,
        samples: vec![0.25; 144_000].into(),
        residency: None,
    }
    .with_complete_source(48_000);
    crate::audio_engine::pitch_runtime_tests::publish_tone(
        &fixture.control,
        0,
        sample.clone(),
        true,
    );
    crate::audio_engine::ordinary_endpoint_reserve_tests::adopt_current_sources(&mut fixture, true);
    crate::audio_engine::ordinary_endpoint_reserve_tests::refresh_actual_runtime_bindings(&fixture);
    (fixture, sample)
}

fn action_request(
    sample: &SampleBuffer,
    permit: crate::audio_engine::native_history_permit::NativeHistoryPermit,
    pitch: PitchIntent,
    request_id: u64,
    epoch: u64,
    target_output_frame: u64,
) -> NativeHistoryRequest {
    let region = FrameRange {
        start: 4800,
        end: 100800,
    };
    let mut playback = SourcePlayback::new(region.start, ExplicitSeekMode::Normal, 1.0);
    assert!(playback.configure(144_000, region));
    NativeHistoryRequest {
        pitch_intent: pitch,
        key_lock: true,
        sample: sample.clone(),
        stems: None,
        permit,
        coverage: None,
        binding: ProductiveSourceBinding::new(sample, 48_000, None),
        playback,
        plan: SourceReadPlan {
            channels: 1,
            sample_frames: 144_000,
            frame_pos: region.start,
            loop_region: region,
            loop_period: None,
            seek_mode: ExplicitSeekMode::Normal,
            selection: StemRenderSelection::full_mix(),
            transition: StemTransition::default(),
        },
        actual_feed: None,
        mapped_input: None,
        target_output_frame,
        request_id,
        epoch,
    }
}

#[test]
fn physical_birth_ordinary_final_guard_rejects_source_history_with_all_action_scalars_equal() {
    let (fixture, sample) = fresh_fixture();
    let start = fixture
        .control
        .engine
        .capture_pad_start(0, 0.1, Some(2.1), 1.0, false, None, None)
        .unwrap();
    start
        .capture_target(
            24_000,
            fixture
                .mixer
                .source_control_revision(start.content_key())
                .unwrap(),
        )
        .unwrap();
    let mut work = start.take_work(None).unwrap().unwrap();
    work.freeze_frames(24_000, 24_000).unwrap();
    let permit = work.capture_history_permit(&sample, 48_000).unwrap();
    let render = FrozenPitchRender {
        pitch: PitchIntent::new(0, 7).unwrap(),
        key_lock: true,
    };
    let request = action_request(
        &sample,
        permit,
        render.pitch,
        work.sequence(),
        work.epoch(),
        24_000,
    );
    work.capture(Some(request), render).unwrap();
    work.prepare().unwrap();
    assert_eq!(work.rt_ready(24_000), Ok(()));
    let native = work.candidate.as_mut().unwrap();
    let address = native.rubberband.as_ref().unwrap().state_address();
    let request = native.source.take().unwrap();
    native.prepare_source_at_endpoint(request).unwrap();
    assert!(!native.has_physical_birth_context());
    assert_eq!(
        work.rt_ready(24_000),
        Err(crate::audio_engine::pitch_actions::PitchActionReject::PreparedMismatch)
    );
    let native = work.candidate.as_mut().unwrap();
    let request = native.source.take().unwrap();
    native.prepare_source_at_physical_birth(request).unwrap();
    assert_eq!(native.rubberband.as_ref().unwrap().state_address(), address);
    assert_eq!(work.rt_ready(24_000), Ok(()));
}

#[test]
fn physical_birth_piano_final_guard_rejects_source_history_with_all_action_scalars_equal() {
    let (fixture, sample) = fresh_fixture();
    let action = fixture.capture(0, "pitch.extra:2:set_retrigger");
    let permit = action
        .capture_history_permit(&sample, 48_000, None)
        .unwrap();
    let request = action_request(
        &sample,
        permit,
        action.render().pitch,
        action.sequence(),
        action.epoch(),
        action.execution_target(),
    );
    let custody = crate::audio_engine::pitch_publication::PitchWorkCapacity::default()
        .claim()
        .unwrap();
    let mut work = PitchActionWork::new(action.clone(), 48_000, 1, custody).unwrap();
    assert!(work.rt_capture(request).is_ok());
    work.prepare().unwrap();
    assert!(work.rt_ready(action.execution_target()).is_ok());
    let native = work.candidate.as_mut().unwrap();
    let address = native.rubberband.as_ref().unwrap().state_address();
    let request = native.source.take().unwrap();
    native.prepare_source_at_endpoint(request).unwrap();
    assert!(!native.has_physical_birth_context());
    assert!(matches!(
        work.rt_ready(action.execution_target()),
        Err(crate::audio_engine::pitch_actions::PitchActionReject::PreparedMismatch)
    ));
    let native = work.candidate.as_mut().unwrap();
    let request = native.source.take().unwrap();
    native.prepare_source_at_physical_birth(request).unwrap();
    assert_eq!(native.rubberband.as_ref().unwrap().state_address(), address);
    assert!(work.rt_ready(action.execution_target()).is_ok());
}
