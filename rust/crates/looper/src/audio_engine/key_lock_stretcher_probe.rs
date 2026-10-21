//! Test-only first mechanism probe: original-source offline stretch versus the independent Live
//! varispeed reference. Profiles, clocks, partitions, geometry and budgets precede measurement.
//! No result selects production behavior or supplies listening/device acceptance.
//!
//! C ABI and offline lifecycle references:
//! https://breakfastquay.com/rubberband/code-doc/rubberband-c_8h_source.html
//! https://breakfastquay.com/rubberband/code-doc/classRubberBand_1_1RubberBandStretcher.html

use super::*;
use crate::audio_engine::audio_quality_metrics::stereo_peak;
use std::ffi::c_int;
use std::marker::PhantomData;
use std::ptr::NonNull;
use std::rc::Rc;
use std::time::Instant;

const PROBE_RATE: u32 = 48_000;
const PROBE_RATIO: f32 = 2.0;
const PROBE_HISTORY: usize = 16_384;
const PROBE_CAPTURE_AFTER_HISTORY: usize = 48_000;
const PROBE_CHANNELS: usize = 2;
const MAX_PROCESS_SIZE: usize = 512;
const MAX_INPUT_FRAMES: usize = 2_000_000;
const MAX_CAPTURE_FRAMES: usize = 1_000_000;
const STUDY_PATTERN: &[usize] = &[512];
const PROCESS_PATTERNS: [(&str, &[usize]); 2] =
    [("512", &[512]), ("irregular", &[73, 511, 7, 512, 127, 3])];
const OFFLINE_TIME_RATIO: f64 = 0.5;
const OFFLINE_PITCH_SCALE: f64 = 1.0;
const THREADING_NEVER: c_int = 0x0001_0000;
const CHANNELS_TOGETHER: c_int = 0x1000_0000;
const ENGINE_FINER: c_int = 0x2000_0000;

#[path = "key_lock_stretcher_probe/accepted_map_comparison.rs"]
mod accepted_map_comparison;

#[derive(Clone, Copy)]
struct OfflineKeyFrames {
    from: [u32; 2],
    to: [u32; 2],
}

#[derive(Clone, Copy)]
struct OfflineConfiguration {
    sample_rate_hz: u32,
    engine: c_int,
    options: c_int,
    time_ratio: f64,
    pitch_scale: f64,
    keyframes: Option<OfflineKeyFrames>,
}

#[repr(C)]
struct StretcherStateOpaque {
    _private: [u8; 0],
}

// The crate's existing build script supplies the same Rubber Band library used by LiveShifter.
// These signatures are the unsigned-int C interface, not the C++ size_t interface.
unsafe extern "C" {
    fn rubberband_new(
        rate: u32,
        channels: u32,
        options: c_int,
        time_ratio: f64,
        pitch_scale: f64,
    ) -> *mut StretcherStateOpaque;
    fn rubberband_delete(state: *mut StretcherStateOpaque);
    fn rubberband_get_engine_version(state: *mut StretcherStateOpaque) -> c_int;
    fn rubberband_get_time_ratio(state: *mut StretcherStateOpaque) -> f64;
    fn rubberband_get_pitch_scale(state: *mut StretcherStateOpaque) -> f64;
    fn rubberband_get_channel_count(state: *mut StretcherStateOpaque) -> u32;
    fn rubberband_get_preferred_start_pad(state: *mut StretcherStateOpaque) -> u32;
    fn rubberband_get_start_delay(state: *mut StretcherStateOpaque) -> u32;
    fn rubberband_set_expected_input_duration(state: *mut StretcherStateOpaque, frames: u32);
    fn rubberband_set_max_process_size(state: *mut StretcherStateOpaque, frames: u32);
    fn rubberband_set_key_frame_map(
        state: *mut StretcherStateOpaque,
        count: u32,
        from: *mut u32,
        to: *mut u32,
    );
    fn rubberband_study(
        state: *mut StretcherStateOpaque,
        input: *const *const f32,
        frames: u32,
        final_block: c_int,
    );
    fn rubberband_process(
        state: *mut StretcherStateOpaque,
        input: *const *const f32,
        frames: u32,
        final_block: c_int,
    );
    fn rubberband_available(state: *mut StretcherStateOpaque) -> c_int;
    fn rubberband_retrieve(
        state: *mut StretcherStateOpaque,
        output: *const *mut f32,
        frames: u32,
    ) -> u32;
    fn rubberband_set_debug_level(state: *mut StretcherStateOpaque, level: c_int);
    fn rubberband_set_default_debug_level(level: c_int);
}

fn emit(record: serde_json::Value) {
    println!("KEY_LOCK_STRETCHER_PROBE {record}");
}

/// Exclusive, non-Send owner. Creation, both passes, retrieval and retirement occur on this
/// offline test thread; no callback, device, application installation or production path is used.
struct OfflineStretcher {
    state: NonNull<StretcherStateOpaque>,
    configuration: OfflineConfiguration,
    variable_keyframes: bool,
    input_frames: usize,
    studied_frames: usize,
    processed_frames: usize,
    study_calls: usize,
    process_calls: usize,
    study_final_calls: usize,
    process_final_calls: usize,
    process_blocks: Vec<usize>,
    _same_thread: PhantomData<Rc<()>>,
}

impl OfflineStretcher {
    fn new(
        configuration: OfflineConfiguration,
        source_frames: usize,
        output_frames: usize,
    ) -> Self {
        Self::new_with_variable_map(configuration, source_frames, output_frames, None)
    }

    /// Additional test-only map path; legacy affine keyframes retain every old assertion.
    fn new_with_variable_map(
        configuration: OfflineConfiguration,
        source_frames: usize,
        output_frames: usize,
        variable_map: Option<&[(u32, u32)]>,
    ) -> Self {
        let OfflineConfiguration {
            sample_rate_hz,
            engine,
            options,
            time_ratio,
            pitch_scale,
            keyframes,
        } = configuration;
        assert!(matches!(engine, 2 | 3));
        assert!((8_000..=192_000).contains(&sample_rate_hz));
        assert_eq!(
            options & 1,
            0,
            "only offline processing belongs to this owner"
        );
        assert_ne!(options & THREADING_NEVER, 0);
        assert_ne!(options & CHANNELS_TOGETHER, 0);
        assert_eq!(options & ENGINE_FINER != 0, engine == 3);
        assert!((1..=MAX_INPUT_FRAMES).contains(&source_frames));
        assert!((1..=MAX_CAPTURE_FRAMES).contains(&output_frames));
        assert!(time_ratio.is_finite() && time_ratio > 0.0);
        assert!(pitch_scale.is_finite() && pitch_scale > 0.0);
        assert_eq!(output_frames as f64 / source_frames as f64, time_ratio);
        let source_frames_c = u32::try_from(source_frames).unwrap();
        let max_process_c = u32::try_from(MAX_PROCESS_SIZE).unwrap();
        let channels_c = u32::try_from(PROBE_CHANNELS).unwrap();
        if let Some(map) = variable_map {
            assert!(configuration.keyframes.is_none());
            assert!((4..=2048).contains(&map.len()));
            let mut previous = (0, 0);
            for &(from, to) in map {
                assert!(from > previous.0 && to > previous.1);
                assert!(from <= source_frames_c && to as usize <= output_frames);
                previous = (from, to);
            }
            assert_eq!(previous, (source_frames_c, output_frames as u32));
        }
        if let Some(map) = keyframes {
            assert!(map.from[0] > 0 && map.from[0] < map.from[1]);
            assert!(map.to[0] > 0 && map.to[0] < map.to[1]);
            assert_eq!(map.from[1], source_frames_c);
            assert_eq!(usize::try_from(map.to[1]).unwrap(), output_frames);
            for index in 0..2 {
                assert_eq!(
                    f64::from(map.to[index]) / f64::from(map.from[index]),
                    time_ratio
                );
            }
        }
        // SAFETY: All scalar inputs have bounded, validated C representations. The constructor
        // returns a newly allocated opaque state; its sole owner below always calls delete.
        let raw = unsafe {
            rubberband_set_default_debug_level(0);
            rubberband_new(sample_rate_hz, channels_c, options, time_ratio, pitch_scale)
        };
        let owner = Self {
            state: NonNull::new(raw).expect("offline Stretcher construction failed"),
            configuration,
            variable_keyframes: variable_map.is_some(),
            input_frames: source_frames,
            studied_frames: 0,
            processed_frames: 0,
            study_calls: 0,
            process_calls: 0,
            study_final_calls: 0,
            process_final_calls: 0,
            process_blocks: Vec::new(),
            _same_thread: PhantomData,
        };
        // The semantic origin is implicitly 0 -> 0. Passing that redundant entry to R3 makes
        // updateRatioFromMap divide its first output/input keyframe as 0/0 before processing.
        // Encode the same frozen geometry with positive native anchors only; no measured shift.
        // https://github.com/breakfastquay/rubberband/blob/v4.0.0/src/finer/R3Stretcher.cpp#L380-L394
        // SAFETY: The handle is live and exclusively owned. Bounded scalar configuration
        // precedes study and process; no PCM or pointer array is retained here.
        unsafe {
            rubberband_set_debug_level(owner.state.as_ptr(), 0);
            rubberband_set_expected_input_duration(owner.state.as_ptr(), source_frames_c);
            rubberband_set_max_process_size(owner.state.as_ptr(), max_process_c);
        }
        if let Some(map) = keyframes {
            let mut from = map.from;
            let mut to = map.to;
            // SAFETY: These two positive, monotonic, fixed geometry anchors were
            // validated before Native construction. The synchronous API copies them.
            unsafe {
                rubberband_set_key_frame_map(
                    owner.state.as_ptr(),
                    u32::try_from(from.len()).unwrap(),
                    from.as_mut_ptr(),
                    to.as_mut_ptr(),
                );
            }
            assert_eq!(from, map.from);
            assert_eq!(to, map.to);
        }
        if let Some(map) = variable_map {
            let mut from = map.iter().map(|point| point.0).collect::<Vec<_>>();
            let mut to = map.iter().map(|point| point.1).collect::<Vec<_>>();
            // SAFETY: bounded positive monotonic C-uint arrays are synchronous inputs;
            // the existing exclusive offline owner and lifecycle are unchanged.
            unsafe {
                rubberband_set_key_frame_map(
                    owner.state.as_ptr(),
                    from.len() as u32,
                    from.as_mut_ptr(),
                    to.as_mut_ptr(),
                );
            }
            assert!(from.iter().zip(&to).eq(map.iter().map(|(a, b)| (a, b))));
        }
        let api = owner.api_profile(engine, options);
        emit(serde_json::json!({ "stage": "api_profile", "api": api }));
        owner
    }

    fn api_profile(&self, requested_engine: c_int, options: c_int) -> serde_json::Value {
        // SAFETY: Query functions only inspect this still-live, exclusively owned handle.
        let (engine, time_ratio, pitch_scale, channels, preferred_pad, start_delay) = unsafe {
            (
                rubberband_get_engine_version(self.state.as_ptr()),
                rubberband_get_time_ratio(self.state.as_ptr()),
                rubberband_get_pitch_scale(self.state.as_ptr()),
                rubberband_get_channel_count(self.state.as_ptr()),
                rubberband_get_preferred_start_pad(self.state.as_ptr()),
                rubberband_get_start_delay(self.state.as_ptr()),
            )
        };
        assert_eq!(
            engine, requested_engine,
            "engine query is R2/R3, not library semver"
        );
        assert!(time_ratio.is_finite() && pitch_scale.is_finite());
        assert_eq!(time_ratio, self.configuration.time_ratio);
        assert_eq!(pitch_scale, self.configuration.pitch_scale);
        assert_eq!(channels, u32::try_from(PROBE_CHANNELS).unwrap());
        assert_eq!(
            preferred_pad, 0,
            "offline auto-padding belongs to the library"
        );
        assert_eq!(
            start_delay, 0,
            "offline automatic alignment defines output time zero"
        );
        serde_json::json!({
            "api": "RubberBandStretcher C ABI",
            "requested_engine": requested_engine,
            "reported_engine": engine,
            "engine_query_is_library_semver": false,
            "options_bits": options,
            "mode": "offline",
            "threading": "never",
            "channels_mode": "together",
            "window": if options & 0x0010_0000 != 0 {"short"} else {"default_standard"},
            "time_ratio": time_ratio,
            "pitch_scale": pitch_scale,
            "channels": channels,
            "preferred_start_pad": preferred_pad,
            "start_delay": start_delay,
            "external_pad_frames": 0,
            "external_output_translation_frames": 0,
            "external_gain": 1.0,
        })
    }

    fn input_pointers(
        &self,
        input: &[Vec<f32>],
        offset: usize,
        frames: usize,
    ) -> [*const f32; PROBE_CHANNELS] {
        assert_eq!(input.len(), PROBE_CHANNELS);
        assert!((1..=MAX_PROCESS_SIZE).contains(&frames));
        let end = offset.checked_add(frames).unwrap();
        assert!(end <= self.input_frames);
        std::array::from_fn(|channel| {
            assert_eq!(input[channel].len(), self.input_frames);
            let block = &input[channel][offset..end];
            assert!(block.iter().all(|value| value.is_finite()));
            block.as_ptr()
        })
    }

    fn study(&mut self, input: &[Vec<f32>], offset: usize, frames: usize) {
        assert_eq!(self.processed_frames, 0);
        assert_eq!(self.study_final_calls, 0);
        assert_eq!(offset, self.studied_frames);
        let pointers = self.input_pointers(input, offset, frames);
        let end = offset.checked_add(frames).unwrap();
        let final_block = end == self.input_frames;
        // SAFETY: The live handle is exclusively borrowed. Both immutable channel blocks have
        // exactly frames valid f32 entries, and their pointer array lasts through this call.
        unsafe {
            rubberband_study(
                self.state.as_ptr(),
                pointers.as_ptr(),
                u32::try_from(frames).unwrap(),
                c_int::from(final_block),
            );
        }
        self.studied_frames = end;
        self.study_calls += 1;
        self.study_final_calls += usize::from(final_block);
    }

    fn process_block(&mut self, input: [&[f32]; PROBE_CHANNELS], offset: usize) {
        assert_eq!(self.studied_frames, self.input_frames);
        assert_eq!(self.study_final_calls, 1);
        assert_eq!(self.process_final_calls, 0);
        assert_eq!(offset, self.processed_frames);
        let frames = input[0].len();
        assert!((1..=MAX_PROCESS_SIZE).contains(&frames));
        let pointers = std::array::from_fn::<_, PROBE_CHANNELS, _>(|channel| {
            assert_eq!(input[channel].len(), frames);
            assert!(input[channel].iter().all(|value| value.is_finite()));
            input[channel].as_ptr()
        });
        let end = offset.checked_add(frames).unwrap();
        assert!(end <= self.input_frames);
        let final_block = end == self.input_frames;
        // SAFETY: The complete study pass already finished. This exclusive live handle and
        // the two bounded immutable input blocks remain valid throughout synchronous process.
        unsafe {
            rubberband_process(
                self.state.as_ptr(),
                pointers.as_ptr(),
                u32::try_from(frames).unwrap(),
                c_int::from(final_block),
            );
        }
        self.processed_frames = end;
        self.process_calls += 1;
        self.process_final_calls += usize::from(final_block);
        self.process_blocks.push(frames);
        if self.process_calls == 1 {
            // Query after the first process call: R3 evaluates the native map there, after the
            // constructor-time query. The declared profile must not silently reset to unity.
            let (time_ratio, pitch_scale) = unsafe {
                (
                    rubberband_get_time_ratio(self.state.as_ptr()),
                    rubberband_get_pitch_scale(self.state.as_ptr()),
                )
            };
            emit(serde_json::json!({
                "stage": "first_process_ratio",
                "time_ratio": time_ratio,
                "pitch_scale": pitch_scale,
            }));
            if self.variable_keyframes {
                assert!(time_ratio.is_finite() && time_ratio > 0.0);
                // The map may change R3's current ratio; no affine-total claim is made.
            } else {
                assert_eq!(time_ratio, self.configuration.time_ratio);
            }
            assert_eq!(pitch_scale, self.configuration.pitch_scale);
        }
    }

    /// Each positive iteration consumes at least one frame. The total output cap bounds this
    /// loop independently of native progress; ThreadingNever requires no polling or sleeps.
    fn drain(&mut self, output: &mut [Vec<f32>], retrieval_calls: &mut usize) -> c_int {
        assert_eq!(output.len(), PROBE_CHANNELS);
        assert_eq!(output[0].len(), output[1].len());
        let mut scratch = [[0.0_f32; MAX_PROCESS_SIZE]; PROBE_CHANNELS];
        for _ in 0..=MAX_CAPTURE_FRAMES {
            // SAFETY: The live handle is exclusive and there is no concurrent native thread.
            let available = unsafe { rubberband_available(self.state.as_ptr()) };
            assert!(available >= -1, "invalid native availability {available}");
            if available <= 0 {
                return available;
            }
            let available = usize::try_from(available).unwrap();
            let remaining_capacity = MAX_CAPTURE_FRAMES.checked_sub(output[0].len()).unwrap();
            assert!(available <= remaining_capacity, "bounded capture exceeded");
            let frames = available.min(MAX_PROCESS_SIZE);
            let pointers = [scratch[0].as_mut_ptr(), scratch[1].as_mut_ptr()];
            // SAFETY: Both distinct scratch channels have MAX_PROCESS_SIZE writable samples.
            // The request is bounded by that capacity and by the immediately queried available
            // count. The pointer array and exclusive live state remain valid during retrieve.
            let retrieved = unsafe {
                rubberband_retrieve(
                    self.state.as_ptr(),
                    pointers.as_ptr(),
                    u32::try_from(frames).unwrap(),
                )
            };
            assert_eq!(usize::try_from(retrieved).unwrap(), frames);
            for channel in 0..PROBE_CHANNELS {
                assert!(
                    scratch[channel][..frames]
                        .iter()
                        .all(|value| value.is_finite())
                );
                output[channel].extend_from_slice(&scratch[channel][..frames]);
            }
            *retrieval_calls += 1;
        }
        panic!("offline capture did not drain within its declared frame bound");
    }
}

impl Drop for OfflineStretcher {
    fn drop(&mut self) {
        // SAFETY: This non-Clone owner deletes its one non-null native handle exactly once,
        // after all synchronous calls have returned, on the same offline test thread.
        unsafe { rubberband_delete(self.state.as_ptr()) };
    }
}

fn offline_capture(
    input: &[Vec<f32>],
    engine: c_int,
    pattern_name: &str,
    process_pattern: &[usize],
    source_target: usize,
    expected_frames: usize,
    canonical_native_blocks: bool,
) -> Vec<Vec<f32>> {
    let source_frames = input[0].len();
    assert!(source_target > 0 && source_target < source_frames);
    assert!(PROBE_HISTORY > 0 && PROBE_HISTORY < expected_frames);
    let configuration = OfflineConfiguration {
        sample_rate_hz: PROBE_RATE,
        engine,
        options: THREADING_NEVER | CHANNELS_TOGETHER | if engine == 3 { ENGINE_FINER } else { 0 },
        time_ratio: OFFLINE_TIME_RATIO,
        pitch_scale: OFFLINE_PITCH_SCALE,
        keyframes: Some(OfflineKeyFrames {
            from: [
                u32::try_from(source_target).unwrap(),
                u32::try_from(source_frames).unwrap(),
            ],
            to: [
                u32::try_from(PROBE_HISTORY).unwrap(),
                u32::try_from(expected_frames).unwrap(),
            ],
        }),
    };
    offline_capture_configured(
        input,
        configuration,
        pattern_name,
        process_pattern,
        expected_frames,
        canonical_native_blocks,
    )
}

fn offline_capture_configured(
    input: &[Vec<f32>],
    configuration: OfflineConfiguration,
    pattern_name: &str,
    process_pattern: &[usize],
    expected_frames: usize,
    canonical_native_blocks: bool,
) -> Vec<Vec<f32>> {
    offline_capture_with_map(
        input,
        configuration,
        pattern_name,
        process_pattern,
        expected_frames,
        canonical_native_blocks,
        None,
    )
}

fn offline_capture_with_map(
    input: &[Vec<f32>],
    configuration: OfflineConfiguration,
    pattern_name: &str,
    process_pattern: &[usize],
    expected_frames: usize,
    canonical_native_blocks: bool,
    variable_map: Option<&[(u32, u32)]>,
) -> Vec<Vec<f32>> {
    let engine = configuration.engine;
    assert!(!process_pattern.is_empty());
    assert!(
        process_pattern
            .iter()
            .all(|frames| (1..=MAX_PROCESS_SIZE).contains(frames))
    );
    assert_eq!(input.len(), PROBE_CHANNELS);
    let source_frames = input[0].len();
    let capture_started = Instant::now();
    emit(serde_json::json!({
        "stage": "begin_offline_owner",
        "engine": engine,
        "process_pattern": pattern_name,
        "canonical_native_blocks": canonical_native_blocks,
    }));
    let mut stretcher = if variable_map.is_some() {
        OfflineStretcher::new_with_variable_map(
            configuration,
            source_frames,
            expected_frames,
            variable_map,
        )
    } else {
        OfflineStretcher::new(configuration, source_frames, expected_frames)
    };
    let construction_elapsed_ms = capture_started.elapsed().as_secs_f64() * 1000.0;
    let study_started = Instant::now();
    let mut offset = 0;
    let mut partition = 0;
    while offset < source_frames {
        let frames = STUDY_PATTERN[partition % STUDY_PATTERN.len()].min(source_frames - offset);
        stretcher.study(input, offset, frames);
        offset += frames;
        partition += 1;
    }
    assert_eq!(stretcher.studied_frames, source_frames);
    assert_eq!(stretcher.study_final_calls, 1);
    let study_elapsed_ms = study_started.elapsed().as_secs_f64() * 1000.0;
    let mut output = (0..PROBE_CHANNELS)
        .map(|_| Vec::with_capacity(expected_frames))
        .collect::<Vec<_>>();
    let mut retrieval_calls = 0;
    let mut terminal_available = 0;
    let mut staging = [[0.0_f32; MAX_PROCESS_SIZE]; PROBE_CHANNELS];
    let mut staged_frames = 0;
    let mut maximum_staged_frames = 0;
    let mut native_offset = 0;
    let mut caller_process_calls = 0;
    offset = 0;
    partition = 0;
    let process_started = Instant::now();
    let mut submit_block = |input_block: [&[f32]; PROBE_CHANNELS], native_offset: usize| {
        stretcher.process_block(input_block, native_offset);
        terminal_available = stretcher.drain(&mut output, &mut retrieval_calls);
        if stretcher.processed_frames < source_frames {
            assert_eq!(
                terminal_available, 0,
                "native completion preceded final input"
            );
        }
    };
    while offset < source_frames {
        let frames = process_pattern[partition % process_pattern.len()].min(source_frames - offset);
        let caller_end = offset.checked_add(frames).unwrap();
        if canonical_native_blocks {
            let mut cursor = offset;
            while cursor < caller_end {
                let count = (MAX_PROCESS_SIZE - staged_frames).min(caller_end - cursor);
                for channel in 0..PROBE_CHANNELS {
                    staging[channel][staged_frames..staged_frames + count]
                        .copy_from_slice(&input[channel][cursor..cursor + count]);
                }
                cursor += count;
                staged_frames += count;
                maximum_staged_frames = maximum_staged_frames.max(staged_frames);
                if staged_frames == MAX_PROCESS_SIZE || cursor == source_frames {
                    for channel in 0..PROBE_CHANNELS {
                        for (staged, original) in staging[channel][..staged_frames]
                            .iter()
                            .zip(&input[channel][native_offset..native_offset + staged_frames])
                        {
                            assert_eq!(staged.to_bits(), original.to_bits());
                        }
                    }
                    submit_block(
                        std::array::from_fn(|channel| &staging[channel][..staged_frames]),
                        native_offset,
                    );
                    native_offset += staged_frames;
                    staged_frames = 0;
                }
            }
        } else {
            submit_block(
                std::array::from_fn(|channel| &input[channel][offset..caller_end]),
                native_offset,
            );
            native_offset += frames;
        }
        offset += frames;
        partition += 1;
        caller_process_calls += 1;
    }
    assert_eq!(native_offset, source_frames);
    assert_eq!(staged_frames, 0);
    if canonical_native_blocks {
        let mut expected_native_blocks = vec![MAX_PROCESS_SIZE; source_frames / MAX_PROCESS_SIZE];
        if !source_frames.is_multiple_of(MAX_PROCESS_SIZE) {
            expected_native_blocks.push(source_frames % MAX_PROCESS_SIZE);
        }
        assert_eq!(stretcher.process_blocks, expected_native_blocks);
        assert!(maximum_staged_frames <= MAX_PROCESS_SIZE);
    }
    let process_and_drain_elapsed_ms = process_started.elapsed().as_secs_f64() * 1000.0;
    emit(serde_json::json!({
        "stage": "offline_completion",
        "engine": engine,
        "process_pattern": pattern_name,
        "study_pattern": STUDY_PATTERN,
        "source_frames": source_frames,
        "studied_frames": stretcher.studied_frames,
        "processed_frames": stretcher.processed_frames,
        "study_calls": stretcher.study_calls,
        "process_calls": stretcher.process_calls,
        "caller_process_calls": caller_process_calls,
        "canonical_native_blocks": canonical_native_blocks,
        "maximum_staged_frames_per_channel": maximum_staged_frames,
        "native_process_block_sequence": stretcher.process_blocks,
        "study_final_calls": stretcher.study_final_calls,
        "process_final_calls": stretcher.process_final_calls,
        "retrieval_calls": retrieval_calls,
        "terminal_available": terminal_available,
        "expected_output_frames": expected_frames,
        "actual_output_frames": [output[0].len(), output[1].len()],
        "full_study_preceded_process": true,
        "elapsed_ms": {
            "construction_and_configuration": construction_elapsed_ms,
            "study": study_elapsed_ms,
            "process_and_drain": process_and_drain_elapsed_ms,
            "capture_before_retirement": capture_started.elapsed().as_secs_f64() * 1000.0,
        },
        "timings_are_observations_without_resource_or_deadline_claim": true,
    }));
    assert_eq!(stretcher.processed_frames, source_frames);
    assert_eq!(stretcher.process_final_calls, 1);
    assert_eq!(
        terminal_available, -1,
        "ThreadingNever final process must finish without polling"
    );
    for channel in &output {
        assert_eq!(
            channel.len(),
            expected_frames,
            "uncropped exact offline duration"
        );
        assert!(channel.iter().all(|value| value.is_finite()));
    }
    drop(stretcher);
    emit(serde_json::json!({
        "stage": "end_offline_owner",
        "engine": engine,
        "process_pattern": pattern_name,
        "native_deleted": true,
        "whole_owner_elapsed_ms": capture_started.elapsed().as_secs_f64() * 1000.0,
    }));
    output
}

fn offsets(times: Option<[i64; 3]>, dry_times: [i64; 3], translation: i64) -> Option<[i64; 3]> {
    times.map(|times| std::array::from_fn(|index| times[index] - dry_times[index] - translation))
}

fn timing_pass(offsets: Option<[i64; 3]>, budget: i64) -> bool {
    offsets.is_some_and(|offsets| offsets[0].abs() <= budget && offsets[1].abs() <= budget)
}

/// Diagnostic gating keeps every sample at its original output coordinate. Only the explicitly
/// declared pre-H samples become zero; no crop, gain, measured shift or threshold fit occurs.
fn report_capture(label: &str, output: &[Vec<f32>], dry_times: [i64; 3], dry_energy: f64) {
    assert_eq!(output.len(), PROBE_CHANNELS);
    assert_eq!(output[0].len(), output[1].len());
    assert!(
        (PROBE_HISTORY + PROBE_RATE as usize / 50..=MAX_CAPTURE_FRAMES).contains(&output[0].len())
    );
    assert!(output.iter().flatten().all(|value| value.is_finite()));
    assert!(dry_energy.is_finite() && dry_energy > 0.0);
    let budget = (f64::from(PROBE_RATE) * TIMING_BUDGET_MS / 1000.0).ceil() as i64;
    let raw_response = response(&output[0]);
    let original_peak = stereo_peak(output);
    let raw_energy = stereo_energy(output);
    let total_energy = raw_energy.iter().sum::<f64>();
    assert!(total_energy.is_finite());
    let has_energy = total_energy > 0.0;
    let raw_times = has_energy.then(|| energy_times(&raw_energy, PROBE_RATE));
    let raw_offsets = offsets(raw_times, dry_times, 0);
    let pre_history_energy = raw_energy[..PROBE_HISTORY].iter().sum::<f64>();
    let discarded_fraction = has_energy.then_some(pre_history_energy / total_energy);
    let tail_frames = PROBE_RATE as usize / 50;
    let tail_energy = raw_energy[raw_energy.len() - tail_frames..]
        .iter()
        .sum::<f64>();
    let tail_fraction = has_energy.then_some(tail_energy / total_energy);
    let energy_limit = has_energy.then(|| maximum_energy_discard(&raw_energy));
    let original_peak_retained = has_energy && original_peak.0 >= PROBE_HISTORY;
    let discard_pass = discarded_fraction.is_some_and(|value| value <= MAX_DISCARDED_ENERGY);
    let tail_pass = tail_fraction.is_some_and(|value| value <= MAX_CAPTURE_TAIL_ENERGY);
    let raw_timing_pass = timing_pass(raw_offsets, budget);
    let mut gated = output.to_vec();
    for channel in &mut gated {
        channel[..PROBE_HISTORY].fill(0.0);
    }
    let gated_energy = stereo_energy(&gated);
    let gated_total = gated_energy.iter().sum::<f64>();
    let gated_times = (gated_total > 0.0).then(|| energy_times(&gated_energy, PROBE_RATE));
    let gated_offsets = offsets(gated_times, dry_times, 0);
    let gated_response = response(&gated[0]);
    let gated_peak = stereo_peak(&gated);
    let original_threshold_onset = (original_peak.1 > 1.0e-7)
        .then(|| {
            (0..gated[0].len()).find(|frame| {
                gated[0][*frame].abs().max(gated[1][*frame].abs()) >= original_peak.1 * 0.01
            })
        })
        .flatten();
    emit(serde_json::json!({
        "stage": "fixed_quality_diagnostic",
        "label": label,
        "output_frames": output[0].len(),
        "output_origin": "uncropped_native_output_frame_0",
        "history_boundary_frames": PROBE_HISTORY,
        "raw": {
            "energy_times_q10_q50_q90": raw_times,
            "q_offsets_from_unchanged_dry": raw_offsets,
            "left_onset": raw_response.onset,
            "left_one_percent_onset": raw_response.significant_onset,
            "left_peak_frame": raw_response.peak_frame,
            "left_peak_amplitude": raw_response.peak,
            "stereo_original_peak_frame": original_peak.0,
            "stereo_original_peak_amplitude": original_peak.1,
            "original_total_energy_denominator": total_energy,
            "unchanged_dry_total_energy": dry_energy,
            "total_energy_over_unchanged_dry_energy": total_energy / dry_energy,
            "energy_fraction_before_H": discarded_fraction,
            "final_20ms_energy_fraction": tail_fraction,
            "maximum_energy_discard_frame": energy_limit,
        },
        "strict_gate_same_full_origin": {
            "all_pre_H_samples_zero": gated.iter().all(|channel| channel[..PROBE_HISTORY].iter().all(|sample| *sample == 0.0)),
            "energy_times_q10_q50_q90": gated_times,
            "q_offsets_from_unchanged_dry": gated_offsets,
            "left_onset": gated_response.onset,
            "stereo_peak_frame": gated_peak.0,
            "stereo_peak_amplitude": gated_peak.1,
            "one_percent_of_ORIGINAL_stereo_peak_onset": original_threshold_onset,
            "one_percent_threshold_ORIGINAL_peak_amplitude": original_peak.1,
            "original_stereo_peak_retained": original_peak_retained,
            "retained_energy_over_ORIGINAL_total": has_energy.then_some(gated_total / total_energy),
            "ORIGINAL_total_energy_denominator": total_energy,
            "gate_changes_only_pre_H": true,
        },
        "fixed_gates": {
            "timing_budget_ms": TIMING_BUDGET_MS,
            "timing_budget_frames": budget,
            "max_discarded_energy": MAX_DISCARDED_ENERGY,
            "max_capture_tail_energy": MAX_CAPTURE_TAIL_ENERGY,
            "raw_q10_q50_timing_pass": raw_timing_pass,
            "strict_gated_q10_q50_timing_pass": timing_pass(gated_offsets, budget),
            "original_peak_retained_pass": original_peak_retained,
            "discarded_energy_pass": discard_pass,
            "capture_tail_pass": tail_pass,
            "raw_criterion_pass": raw_timing_pass && original_peak_retained && discard_pass && tail_pass,
            "strict_gated_criterion_pass": timing_pass(gated_offsets, budget) && original_peak_retained && discard_pass && tail_pass,
        },
        "product_or_listening_acceptance_claim": false,
    }));
}

fn report_live_translation(reference: &NativeReference, dry_times: [i64; 3]) {
    let energy = stereo_energy(&reference.output);
    let raw_times = energy_times(&energy, PROBE_RATE);
    let raw_peak = stereo_peak(&reference.output);
    let energy_limit = maximum_energy_discard(&energy);
    let total_energy = energy.iter().sum::<f64>();
    let budget = (f64::from(PROBE_RATE) * TIMING_BUDGET_MS / 1000.0).ceil() as i64;
    let centers = offsets(Some(raw_times), dry_times, 0).unwrap();
    let timing_lower = (centers[0] - budget).max(centers[1] - budget);
    let timing_upper = (centers[0] + budget).min(centers[1] + budget);
    let retention_upper = raw_peak.0.min(energy_limit) as i64 - PROBE_HISTORY as i64;
    let lower = timing_lower.max(-(PROBE_HISTORY as i64));
    let upper = timing_upper.min(retention_upper);
    let nominal_discard = PROBE_HISTORY.checked_add(reference.delay).unwrap();
    assert!(nominal_discard < energy.len());
    let nominal_fraction = energy[..nominal_discard].iter().sum::<f64>() / total_energy;
    emit(serde_json::json!({
        "stage": "live_clock_translation_diagnostic",
        "live_input": "independent_fixture_reference_sample_varispeed_r",
        "live_pitch_scale": 1.0 / f64::from(PROBE_RATIO),
        "raw_output_origin_translation_frames": 0,
        "native_delay": reference.delay,
        "native_block_size": reference.block_size,
        "raw_CAPTURE_frames": reference.output[0].len(),
        "native_reference_duration_rule": "ceil((delay + requested_frames) / block) * block; all retained",
        "illustrative_API_delay_translation": {
            "C_frames": reference.delay,
            "raw_frame_corresponding_to_nominal_H": nominal_discard,
            "q_offsets_from_dry_if_clock_translated": offsets(Some(raw_times), dry_times, reference.delay as i64),
            "original_peak_retained_if_strict_gate_at_H_plus_C": raw_peak.0 >= nominal_discard,
            "discarded_energy_fraction_at_H_plus_C": nominal_fraction,
            "discarded_energy_pass": nominal_fraction <= MAX_DISCARDED_ENERGY,
            "required_headroom_over_retention_limit_frames": (reference.delay as i64 - retention_upper).max(0),
            "accepted_correction": false,
        },
        "raw_coordinate_translation_feasibility": {
            "C_timing_lower_frames": timing_lower,
            "C_timing_upper_frames": timing_upper,
            "U_retention_upper_frames": retention_upper,
            "intersection_lower_frames": lower,
            "intersection_upper_frames": upper,
            "incompatibility_gap_frames": (lower - upper).max(0),
            "any_integer_translation_feasible": lower <= upper,
            "history_access_is_not_clock_compensation": true,
        },
        "previous_same_phase_hard_negative": {
            "C_nominal_API_frames": 3678,
            "U_retention_frames": 2080,
            "gap_frames": 1598,
            "current_matches": reference.delay == 3678 && retention_upper == 2080 && reference.delay as i64 - retention_upper == 1598,
        },
        "offline_autoalignment_is_a_different_clock_contract": true,
        "product_or_listening_acceptance_claim": false,
    }));
}

fn compare_complete_pcm(
    engine: c_int,
    comparison: &str,
    left: &[Vec<f32>],
    right: &[Vec<f32>],
    expected_frames: usize,
) -> (bool, usize, f64) {
    assert_eq!(left.len(), PROBE_CHANNELS);
    assert_eq!(right.len(), PROBE_CHANNELS);
    let mut mismatched_samples = 0;
    let mut max_sample_error = 0.0_f64;
    for channel in 0..PROBE_CHANNELS {
        assert_eq!(left[channel].len(), expected_frames);
        assert_eq!(right[channel].len(), expected_frames);
        for (left, right) in left[channel].iter().zip(&right[channel]) {
            mismatched_samples += usize::from(left.to_bits() != right.to_bits());
            max_sample_error = max_sample_error.max((f64::from(*left) - f64::from(*right)).abs());
        }
    }
    let equal = mismatched_samples == 0;
    emit(serde_json::json!({
        "stage": "full_pcm_partition_contract",
        "engine": engine,
        "comparison": comparison,
        "frames_per_channel_compared": expected_frames,
        "channels_compared": PROBE_CHANNELS,
        "full_pcm_bitwise_equal": equal,
        "mismatched_samples": mismatched_samples,
        "maximum_absolute_sample_error": max_sample_error,
        "fitted_tolerance": 0.0,
    }));
    (equal, mismatched_samples, max_sample_error)
}

#[test]
fn original_source_offline_stretcher_r2_r3_first_mechanism_probe() {
    assert_eq!(TIMING_BUDGET_MS, 2.0);
    assert_eq!(MAX_DISCARDED_ENERGY, 0.001);
    assert_eq!(MAX_CAPTURE_TAIL_ENERGY, 0.000_001);
    let fixture = marker_fixture(
        PROBE_RATE,
        PROBE_RATIO,
        PROBE_HISTORY,
        0,
        Signal::Impulse,
        PROBE_HISTORY
            .checked_add(PROBE_CAPTURE_AFTER_HISTORY)
            .unwrap(),
    );
    assert_eq!(fixture.sample.channels, PROBE_CHANNELS);
    assert_eq!(fixture.sample.samples.len() % PROBE_CHANNELS, 0);
    let source_frames = fixture.sample.samples.len() / PROBE_CHANNELS;
    assert_eq!(source_frames % 2, 0, "q=.5 must have an integral duration");
    let expected_frames = source_frames / 2;
    let source_target = PROBE_HISTORY.checked_mul(2).unwrap();
    assert_eq!(
        expected_frames as f64 / source_frames as f64,
        OFFLINE_TIME_RATIO
    );
    assert_eq!(
        source_target as f64 * OFFLINE_TIME_RATIO,
        PROBE_HISTORY as f64
    );
    assert_eq!(fixture.start, 0);
    assert_eq!(fixture.region.start, 0);
    assert_eq!(fixture.region.end, source_frames);
    assert!(fixture.stems.is_none() && fixture.stem_mask.is_none());
    for (frame, channels) in fixture
        .sample
        .samples
        .chunks_exact(PROBE_CHANNELS)
        .enumerate()
    {
        let expected = if frame == source_target {
            [1.0, 0.5]
        } else {
            [0.0, 0.0]
        };
        assert_eq!(channels, expected, "one immutable original-source impulse");
    }
    let original_planar = (0..PROBE_CHANNELS)
        .map(|channel| {
            fixture
                .sample
                .samples
                .chunks_exact(PROBE_CHANNELS)
                .map(|frame| frame[channel])
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let dry = (0..PROBE_CHANNELS)
        .map(|channel| {
            (0..expected_frames)
                .map(|frame| fixture.reference_sample(frame, channel))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    // This declaration is emitted before constructing or observing either offline engine or Live.
    emit(serde_json::json!({
        "stage": "predeclared_probe_contract",
        "sample_rate": PROBE_RATE,
        "tempo_ratio_r": PROBE_RATIO,
        "neutral_pitch_h": 1.0,
        "signal": "impulse",
        "history_frames_H": PROBE_HISTORY,
        "marker": 0,
        "fixture_capture_frames": PROBE_HISTORY + PROBE_CAPTURE_AFTER_HISTORY,
        "original_source_frames": source_frames,
        "offline_expected_output_frames": expected_frames,
        "offline_q": OFFLINE_TIME_RATIO,
        "offline_p": OFFLINE_PITCH_SCALE,
        "live_p": 1.0 / f64::from(PROBE_RATIO),
        "original_source_to_offline_output_keyframes": [[0, 0], [source_target, PROBE_HISTORY], [source_frames, expected_frames]],
        "native_positive_keyframes": [[source_target, PROBE_HISTORY], [source_frames, expected_frames]],
        "semantic_zero_origin_is_implicit_not_a_native_zero_divisor": true,
        "keyframes_are_fixed_geometry_not_measurement_fit": true,
        "engines": [2, 3],
        "offline_profile": {
            "mode": "offline",
            "threading": "never",
            "channels_mode": "together",
            "window": "default_standard",
            "R2_options_bits": THREADING_NEVER | CHANNELS_TOGETHER,
            "R3_options_bits": THREADING_NEVER | CHANNELS_TOGETHER | ENGINE_FINER,
            "max_process_size": MAX_PROCESS_SIZE,
        },
        "study_pattern": STUDY_PATTERN,
        "process_patterns": [[512], [73, 511, 7, 512, 127, 3]],
        "partition_contract": "full PCM bitwise equality; zero fitted tolerance; failed direct-native route remains rejected",
        "offline_ingress_adapter": {
            "staging_capacity_frames_per_channel": MAX_PROCESS_SIZE,
            "native_block_size": MAX_PROCESS_SIZE,
            "final_remainder": source_frames % MAX_PROCESS_SIZE,
            "complete_output_comparisons": ["adapter512_vs_adapter_irregular", "each_adapter_vs_direct512"],
            "direct_native_failure_is_not_adapter_success": true,
        },
        "output_mapping": "offline automatic alignment; no user offset, crop or gain",
        "limits": {
            "max_input_frames": MAX_INPUT_FRAMES,
            "max_capture_frames": MAX_CAPTURE_FRAMES,
            "timing_budget_ms": TIMING_BUDGET_MS,
            "max_discarded_energy": MAX_DISCARDED_ENERGY,
            "max_capture_tail_energy": MAX_CAPTURE_TAIL_ENERGY,
        },
        "production_activation": false,
        "single_impulse_establishes_source_pitch_accuracy": false,
        "nonzero_variable_pitch_and_15_cent_acceptance_gates_remain_open": true,
    }));
    let dry_times = energy_times(&stereo_energy(&dry), PROBE_RATE);
    let dry_energy = stereo_energy(&dry).iter().sum::<f64>();
    assert_eq!(stereo_peak(&dry), (PROBE_HISTORY, 1.0));
    #[cfg(windows)]
    {
        let identity = crate::audio_engine::pitch_runtime_tests::native_identity_tests::identify_loaded_native()
            .expect("identify already loaded Live import without loading or replacing a DLL");
        emit(serde_json::json!({
            "stage": "already_loaded_native_identity",
            "binding_scope": "existing Live import; not Stretcher engine query or library semver",
            "identity": identity,
        }));
    }
    report_capture(
        "unchanged_dry_varispeed_reference",
        &dry,
        dry_times,
        dry_energy,
    );
    let mut adapter_contracts = Vec::new();
    for engine in [2, 3] {
        let captures = PROCESS_PATTERNS.map(|(name, pattern)| {
            let output = offline_capture(
                &original_planar,
                engine,
                name,
                pattern,
                source_target,
                expected_frames,
                false,
            );
            report_capture(
                &format!("original_source_offline_R{engine}_{name}"),
                &output,
                dry_times,
                dry_energy,
            );
            output
        });
        // This raw native comparison remains visible even when rejected. It does not supply
        // the adapter's independent caller-partition guarantee below.
        compare_complete_pcm(
            engine,
            "direct_native_512_vs_irregular",
            &captures[0],
            &captures[1],
            expected_frames,
        );
        let adapted = PROCESS_PATTERNS.map(|(name, pattern)| {
            let output = offline_capture(
                &original_planar,
                engine,
                name,
                pattern,
                source_target,
                expected_frames,
                true,
            );
            report_capture(
                &format!("original_source_offline_R{engine}_adapter_{name}"),
                &output,
                dry_times,
                dry_energy,
            );
            output
        });
        adapter_contracts.push((
            engine,
            "adapter_512_vs_irregular",
            compare_complete_pcm(
                engine,
                "adapter_512_vs_irregular",
                &adapted[0],
                &adapted[1],
                expected_frames,
            ),
        ));
        for (index, (name, _)) in PROCESS_PATTERNS.iter().enumerate() {
            let comparison = format!("adapter_{name}_vs_direct_512");
            let result = compare_complete_pcm(
                engine,
                &comparison,
                &adapted[index],
                &captures[0],
                expected_frames,
            );
            adapter_contracts.push((engine, "adapter_vs_direct_fixed_reference", result));
        }
    }
    emit(serde_json::json!({ "stage": "begin_independent_Live_reference_owner" }));
    let live = native_reference(&fixture, 0, expected_frames);
    emit(serde_json::json!({
        "stage": "end_independent_Live_reference_owner",
        "native_deleted": true,
    }));
    report_capture(
        "independent_Live_uncropped_varispeed_plus_inverse_pitch",
        &live.output,
        dry_times,
        dry_energy,
    );
    report_live_translation(&live, dry_times);
    for (engine, comparison, (equal, mismatches, error)) in adapter_contracts {
        assert!(
            equal,
            "R{engine} {comparison} full PCM partition contract failed: {mismatches} samples; max error {error}"
        );
    }
}

#[path = "key_lock_stretcher_probe/hard_negative_tests.rs"]
mod hard_negative_tests;
