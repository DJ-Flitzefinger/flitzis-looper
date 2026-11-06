//! Exercise the vendor release boundary with the original Looper mixer and spool.

use super::*;
use crate::audio_engine::mix_capture::{SpoolState, inspect_spool};
use cpal::platform::WasapiStream;
use std::sync::mpsc;

fn wasapi_renderer(
    mut stream: Stream,
    submission: Arc<OutputSubmission>,
    (channels, left, right): (usize, usize, usize),
) -> (AudioBufferRetirementWorker, CpalOutputRenderer) {
    let fault = CpalOutputFault {
        stream_failed: Arc::default(),
        admission_unavailable: stream.ownership.output_unavailable.clone(),
        clock: stream.owner.state_mut().callback_clock.clone(),
        submission: Some(submission),
    };
    (
        stream.retirement,
        CpalOutputRenderer::new(
            stream.owner,
            InputClock::new(),
            channels,
            left,
            right,
            RATE,
            fault,
        ),
    )
}

fn wasapi_packet(
    renderer: &mut CpalOutputRenderer,
    pcm: &mut [f32],
    frames: u32,
    release: impl FnOnce(u32) -> Result<(), cpal::StreamError>,
) -> Result<bool, cpal::StreamError> {
    // The backing slice lives through the synchronous vendor render/release call.
    let mut data = unsafe {
        cpal::Data::from_parts(pcm.as_mut_ptr().cast(), pcm.len(), cpal::SampleFormat::F32)
    };
    let info = cpal::OutputCallbackInfo::new(cpal::OutputStreamTimestamp {
        callback: cpal::StreamInstant::new(0, 0),
        playback: cpal::StreamInstant::new(0, 1_000_000),
    });
    let fault = renderer.fault.clone();
    let result = WasapiStream::test_render_and_release(
        &mut data,
        &info,
        frames,
        |data, info, frames| renderer.render(data, info, Some(frames)),
        release,
        |receipt| fault.submitted(receipt),
    );
    if result.is_err() {
        fault.signal();
    }
    result
}

#[test]
fn wasapi_submission_covers_actual_mixer_capture_only_after_release() {
    let sources = sources(1);
    for (layout, fail_release) in [((2, 0, 1), false), ((4, 3, 1), false), ((2, 0, 1), true)] {
        let submission = Arc::new(OutputSubmission::default());
        let mut stream = Stream::new(&sources, false);
        let mut take = stream.record_for_output(17, 83, Some(submission.clone()));
        let (mut retirement, mut renderer) = wasapi_renderer(stream, submission.clone(), layout);
        let mut output = vec![1.0; FRAMES * layout.0];
        let release_result = wasapi_packet(&mut renderer, &mut output, FRAMES as u32, |released| {
            assert_eq!(released, 96);
            let deadline = Instant::now() + WAIT;
            loop {
                let status = take.control.status();
                assert!(!status.terminal_decided && status.payload_sha256.is_none());
                assert_eq!(submission.prefix(), (0, false));
                if status.rt_sealed && status.written_frames == 66 {
                    break;
                }
                assert!(Instant::now() < deadline, "{status:?}");
                std::thread::sleep(PERIOD);
            }
            if fail_release {
                return Err(cpal::StreamError::DeviceNotAvailable);
            }
            Ok(())
        });
        assert_eq!(renderer.owner.state_mut().transport.output_frame(), 96);
        if fail_release {
            assert!(release_result.is_err());
            assert_eq!(submission.prefix(), (0, true));
        } else {
            assert!(release_result.unwrap());
            assert_eq!(submission.prefix(), (96, false));

            // The actual second packet size is independent of the initial period.
            let mut short = vec![0.0; 48 * layout.0];
            assert!(
                wasapi_packet(&mut renderer, &mut short, 48, |released| {
                    assert_eq!(released, 48);
                    assert_eq!(submission.prefix(), (96, false));
                    Ok(())
                })
                .unwrap()
            );
            assert_eq!(submission.prefix(), (144, false));
            assert!(
                wasapi_packet(&mut renderer, &mut output.clone(), 96, |_| {
                    Err(cpal::StreamError::DeviceNotAvailable)
                })
                .is_err()
            );
            assert_eq!(submission.prefix(), (144, true));
        }
        let status = take.joined();
        assert_eq!((status.first_frame, status.end_frame), (Some(17), Some(83)));
        assert_eq!(status.written_frames, 66);
        assert_eq!(status.successful_spool(), !fail_release);
        assert_eq!(
            status.failure,
            fail_release.then_some(CaptureFailure::Stream)
        );
        let spool = inspect_spool(&take.path).unwrap();
        assert_eq!(
            spool.state,
            if fail_release {
                SpoolState::FailedIncomplete
            } else {
                SpoolState::SealedCandidate
            }
        );
        let (_, left, right) = layout;
        if layout.0 == 4 {
            assert!(
                output
                    .chunks_exact(4)
                    .all(|frame| frame[0] == 0.0 && frame[2] == 0.0)
            );
        }
        let expected: Vec<_> = output
            .chunks_exact(layout.0)
            .skip(17)
            .take(66)
            .flat_map(|frame| [frame[left], frame[right]])
            .collect();
        assert!(expected.iter().any(|sample| sample.abs() > 1.0e-3));
        assert!(expected.chunks_exact(2).any(|frame| frame[0] != frame[1]));
        let bytes = std::fs::read(&take.path).unwrap();
        assert!(
            bytes[SPOOL_HEADER_BYTES as usize..]
                .chunks_exact(4)
                .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
                .eq(expected.iter().map(|sample| sample.to_bits()))
        );
        drop(renderer);
        retirement.join_checked().unwrap();
    }
}

#[test]
fn wasapi_submission_refuses_incomplete_geometry_and_a_gap() {
    for gap in [false, true] {
        let submission = Arc::new(OutputSubmission::default());
        let (mut retirement, mut renderer) =
            wasapi_renderer(Stream::new(&[], false), submission.clone(), (2, 0, 1));
        if gap {
            renderer
                .owner
                .state_mut()
                .transport
                .advance_by_rendered_frames(96);
        }
        let mut pcm = [0.0; FRAMES * 2];
        let frames_available = if gap { 96 } else { 95 };
        let processed = wasapi_packet(&mut renderer, &mut pcm, frames_available, |released| {
            assert_eq!(released, if gap { 96 } else { 0 });
            Ok(())
        });
        assert!(!processed.unwrap());
        assert_eq!(submission.prefix(), (0, true));
        assert!(renderer.fault.stream_failed.load(Ordering::Acquire));
        assert_eq!(
            renderer.owner.state_mut().transport.output_frame(),
            if gap { 192 } else { 0 }
        );
        drop(renderer);
        retirement.join_checked().unwrap();
    }
}

#[test]
fn wasapi_checked_close_joins_the_disconnected_reader() {
    let submission = Arc::new(OutputSubmission::default());
    let (mut retirement, renderer) =
        wasapi_renderer(Stream::new(&[], false), submission.clone(), (2, 0, 1));
    let (disconnected_tx, disconnected_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let mut stream = WasapiStream::test_spawn_disconnected(move || {
        disconnected_tx.send(()).unwrap();
        release_rx.recv_timeout(WAIT).unwrap();
        drop(renderer);
    });
    disconnected_rx.recv_timeout(WAIT).unwrap();
    let (closing_tx, closing_rx) = mpsc::channel();
    let (closed_tx, closed_rx) = mpsc::channel();
    std::thread::scope(|scope| {
        scope.spawn(|| {
            closing_tx.send(()).unwrap();
            stream.close_checked().unwrap();
            closed_tx.send(()).unwrap();
        });
        closing_rx.recv_timeout(WAIT).unwrap();
        let before_reader_end = closed_rx.recv_timeout(Duration::from_millis(30));
        let prefix_before_reader_end = submission.prefix();
        release_tx.send(()).unwrap();
        assert_eq!(before_reader_end, Err(mpsc::RecvTimeoutError::Timeout));
        assert_eq!(prefix_before_reader_end, (0, false));
        closed_rx.recv_timeout(WAIT).unwrap();
        assert_eq!(submission.prefix(), (0, true));
    });
    stream.close_checked().unwrap();
    retirement.join_checked().unwrap();
}
