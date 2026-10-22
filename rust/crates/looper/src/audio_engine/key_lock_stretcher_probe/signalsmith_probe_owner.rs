//! Exclusive off-realtime C ABI owner and one frozen finite ending protocol.

use super::*;

const WINDOW: usize = 5292;
const INTERVAL: usize = 1323;
const INPUT_LATENCY: usize = 2646;
const OUTPUT_LATENCY: usize = 2646;
const DELAY: usize = INPUT_LATENCY + OUTPUT_LATENCY;
const FLUSH: usize = OUTPUT_LATENCY + 2 * WINDOW;
const MAX_CALLS: usize = MAX_INPUT_FRAMES.div_ceil(MAX_PROCESS_SIZE);
const BUDGET: usize = 256 * 1024 * 1024;

#[repr(C)]
struct ProbeOpaque {
    _private: [u8; 0],
}

#[repr(C)]
#[derive(Debug, Default, PartialEq)]
struct Info {
    version_major: u32,
    version_minor: u32,
    version_patch: u32,
    channels: u32,
    sample_rate: u32,
    block_samples: u32,
    interval_samples: u32,
    input_latency: u32,
    output_latency: u32,
    split_computation: u32,
    seed: u32,
}

unsafe extern "C" {
    fn flitzis_signalsmith_probe_create(
        pitch: f64,
        info: *mut Info,
        owner: *mut *mut ProbeOpaque,
    ) -> i32;
    fn flitzis_signalsmith_probe_process(
        owner: *mut ProbeOpaque,
        left: *const f32,
        right: *const f32,
        out_left: *mut f32,
        out_right: *mut f32,
        frames: u32,
    ) -> i32;
    fn flitzis_signalsmith_probe_flush(
        owner: *mut ProbeOpaque,
        out_left: *mut f32,
        out_right: *mut f32,
        frames: u32,
    ) -> i32;
    fn flitzis_signalsmith_probe_destroy(owner: *mut ProbeOpaque);
}

struct Probe {
    state: NonNull<ProbeOpaque>,
    terminal: bool,
    processed: usize,
    _same_thread: PhantomData<Rc<()>>,
}

impl Probe {
    fn new(pitch: f64) -> Result<Self, i32> {
        if pitch != 1.0 && pitch != 2.0 {
            return Err(1);
        }
        let mut info = Info::default();
        let mut raw = std::ptr::null_mut();
        // SAFETY: Live exclusive out-parameters; the C++ factory catches every exception
        // and retains no borrowed pointer. Its successful pointer immediately acquires RAII.
        let status = unsafe { flitzis_signalsmith_probe_create(pitch, &mut info, &mut raw) };
        if status != 0 {
            return Err(status);
        }
        let owner = Self {
            state: NonNull::new(raw).ok_or(3)?,
            terminal: false,
            processed: 0,
            _same_thread: PhantomData,
        };
        assert_eq!(
            info,
            Info {
                version_major: 1,
                version_minor: 3,
                version_patch: 2,
                channels: PROBE_CHANNELS as u32,
                sample_rate: RATE,
                block_samples: WINDOW as u32,
                interval_samples: INTERVAL as u32,
                input_latency: INPUT_LATENCY as u32,
                output_latency: OUTPUT_LATENCY as u32,
                split_computation: 0,
                seed: 0,
            },
            "unexpected pre-signal API; do not fit latency or change the frozen profile"
        );
        emit(serde_json::json!({
            "stage":"signalsmith_pre_signal_api","version":[1,3,2],
            "channels":info.channels,"sample_rate_hz":info.sample_rate,
            "block_samples":info.block_samples,"interval_samples":info.interval_samples,
            "input_latency_Li":info.input_latency,"output_latency_Lo":info.output_latency,
            "D_Li_plus_Lo":DELAY,"P":0,"q":1.0,"p":pitch,
            "seed":0,"random_engine":"std::mt19937","fresh_owner":true,
            "reset_reseeds_random_engine":false,"split_computation":false,
            "preset":"presetDefault","FFT":"builtin portable fallback",
            "queried_before_signal":true,"nominal_axis":"raw_n-(Li+Lo)",
            "seek_outputSeek_exact_crop_gain_fit":false
        }));
        Ok(owner)
    }

    fn process(
        &mut self,
        input: &[[f32; MAX_PROCESS_SIZE]; PROBE_CHANNELS],
        output: &mut [[f32; MAX_PROCESS_SIZE]; PROBE_CHANNELS],
        frames: usize,
    ) -> Result<(), i32> {
        if self.terminal {
            return Err(4);
        }
        if !(1..=MAX_PROCESS_SIZE).contains(&frames)
            || self
                .processed
                .checked_add(frames)
                .is_none_or(|n| n > MAX_INPUT_FRAMES)
            || input
                .iter()
                .any(|c| c[..frames].iter().any(|n| !n.is_finite()))
        {
            return Err(1);
        }
        self.terminal = true;
        let left = output[0].as_mut_ptr();
        let right = output[1].as_mut_ptr();
        // SAFETY: Two distinct bounded input arrays and two distinct writable output
        // arrays remain borrowed for the call; no overlap or retained pointers exists.
        let status = unsafe {
            flitzis_signalsmith_probe_process(
                self.state.as_ptr(),
                input[0].as_ptr(),
                input[1].as_ptr(),
                left,
                right,
                u32::try_from(frames).unwrap(),
            )
        };
        if status != 0 {
            return Err(status);
        }
        self.processed += frames;
        self.terminal = false;
        Ok(())
    }

    fn flush(&mut self, output: &mut [Vec<f32>]) -> Result<(), i32> {
        if self.terminal {
            return Err(4);
        }
        if output.len() != PROBE_CHANNELS
            || output.iter().any(|c| c.len() != FLUSH)
            || self
                .processed
                .checked_add(FLUSH - INTERVAL)
                .is_none_or(|n| n > MAX_INPUT_FRAMES)
        {
            return Err(1);
        }
        self.terminal = true;
        let left = output[0].as_mut_ptr();
        let right = output[1].as_mut_ptr();
        // SAFETY: Independently owned channel Vecs each contain exactly FLUSH writable
        // samples. This sole final call uses explicit playbackRate=1 in the C++ bridge.
        let status = unsafe {
            flitzis_signalsmith_probe_flush(self.state.as_ptr(), left, right, FLUSH as u32)
        };
        if status != 0 {
            return Err(status);
        }
        Ok(())
    }
}

impl Drop for Probe {
    fn drop(&mut self) {
        // SAFETY: One non-Clone/non-Send/non-Sync owner destroys its live handle once,
        // after all borrows and synchronous calls end, on the original offline thread.
        unsafe { flitzis_signalsmith_probe_destroy(self.state.as_ptr()) };
    }
}

pub(super) fn pcm_bytes(channels: &[Vec<f32>]) -> usize {
    channels
        .iter()
        .try_fold(std::mem::size_of_val(channels), |sum, c| {
            sum.checked_add(c.capacity().checked_mul(std::mem::size_of::<f32>())?)
        })
        .unwrap()
}

pub(super) fn admit_arrays(resident: usize, frames: usize, previous: Option<&[Vec<f32>]>) {
    assert!((1..=MAX_INPUT_FRAMES - INPUT_LATENCY - FLUSH).contains(&frames));
    let raw = frames.checked_add(DELAY + 2 * WINDOW).unwrap();
    assert!(raw <= MAX_CAPTURE_FRAMES);
    // Current Raw reserves at most 2R per channel. The previous comparison capture
    // stays resident. Sum all shared scorer temporaries even when not simultaneous.
    let current = raw
        .checked_mul(2 * PROBE_CHANNELS * std::mem::size_of::<f32>())
        .unwrap();
    let scorer = raw
        .checked_mul(128)
        .and_then(|n| n.checked_add(frames.checked_mul(56)?))
        .and_then(|n| n.checked_add(1104))
        .unwrap();
    let peak = resident
        .checked_add(current)
        .and_then(|n| n.checked_add(previous.map_or(0, pcm_bytes)))
        .and_then(|n| n.checked_add(scorer))
        .and_then(|n| n.checked_add(2 * FLUSH * PROBE_CHANNELS * std::mem::size_of::<f32>()))
        .and_then(|n| n.checked_add(MAX_CALLS * std::mem::size_of::<usize>() * 2))
        .and_then(|n| {
            n.checked_add(2 * PROBE_CHANNELS * MAX_PROCESS_SIZE * std::mem::size_of::<f32>())
        })
        .and_then(|n| n.checked_add(64 * std::mem::size_of::<Vec<f32>>()))
        .unwrap();
    assert!(peak <= BUDGET);
    emit(serde_json::json!({
        "stage":"signalsmith_pre_growth_array_budget","canonical_frames":frames,
        "resident_source_shifted_canonical_capacity_and_headers_bytes":resident,
        "previous_Raw_capacity_and_headers_bytes":previous.map_or(0,pcm_bytes),
        "current_Raw_channel_capacity_bound":2*raw,"scorer_arrays_bytes":scorer,
        "scorer_formula":"128R+56C+1104; all nonoverlapping temporaries summed",
        "peak_owned_array_bound_bytes":peak,"budget_bytes":BUDGET,
        "opaque_Native_RAM_allocator_JSON_RSS_callback_resource216_included":false
    }));
}

pub(super) fn capture_signalsmith(
    input: &[Vec<f32>],
    pitch: f64,
    caller: (&str, &[usize]),
    resident: usize,
    previous: Option<&[Vec<f32>]>,
) -> Capture {
    assert_eq!(input.len(), PROBE_CHANNELS);
    let frames = input[0].len();
    assert!(
        input
            .iter()
            .all(|c| c.len() == frames && c.iter().all(|n| n.is_finite()))
    );
    assert!(!caller.1.is_empty() && caller.1.iter().all(|n| (1..=MAX_PROCESS_SIZE).contains(n)));
    admit_arrays(resident, frames, previous);
    let total = frames.checked_add(INPUT_LATENCY).unwrap();
    let raw_frames = total.checked_add(FLUSH).unwrap();
    let mut output = (0..PROBE_CHANNELS)
        .map(|_| {
            let mut c = Vec::new();
            c.try_reserve_exact(raw_frames)
                .expect("bounded Raw allocation");
            assert!(c.capacity() <= 2 * raw_frames);
            c
        })
        .collect::<Vec<_>>();
    let mut ending = (0..PROBE_CHANNELS)
        .map(|_| vec![0.0_f32; FLUSH])
        .collect::<Vec<_>>();
    assert!(ending.iter().all(|c| c.capacity() <= 2 * FLUSH));
    let mut blocks = Vec::new();
    blocks
        .try_reserve_exact(total.div_ceil(MAX_PROCESS_SIZE))
        .unwrap();
    assert!(blocks.capacity() <= 2 * MAX_CALLS);
    let mut owner = Probe::new(pitch).expect("exclusive Signalsmith owner");
    let mut staged = [[0.0_f32; MAX_PROCESS_SIZE]; PROBE_CHANNELS];
    let mut wet = [[0.0_f32; MAX_PROCESS_SIZE]; PROBE_CHANNELS];
    let mut cursor = 0;
    let mut pending = 0;
    let mut calls = 0;
    while cursor < total {
        assert!(
            calls < MAX_INPUT_FRAMES,
            "independent positive-progress caller bound"
        );
        let count = caller.1[calls % caller.1.len()].min(total - cursor);
        for _ in 0..count {
            for channel in 0..PROBE_CHANNELS {
                staged[channel][pending] = if cursor < frames {
                    input[channel][cursor]
                } else {
                    0.0
                };
            }
            cursor += 1;
            pending += 1;
            if pending == MAX_PROCESS_SIZE || cursor == total {
                assert!(blocks.len() < MAX_CALLS);
                owner
                    .process(&staged, &mut wet, pending)
                    .expect("bounded process");
                for channel in 0..PROBE_CHANNELS {
                    assert!(output[channel].len() + pending <= raw_frames);
                    output[channel].extend_from_slice(&wet[channel][..pending]);
                }
                blocks.push(pending);
                pending = 0;
            }
        }
        calls += 1;
    }
    assert_eq!(owner.processed, total);
    assert_eq!(pending, 0);
    assert_eq!(blocks.len(), total.div_ceil(MAX_PROCESS_SIZE));
    assert!(
        blocks[..blocks.len() - 1]
            .iter()
            .all(|n| *n == MAX_PROCESS_SIZE)
    );
    assert_eq!(*blocks.last().unwrap(), (total - 1) % MAX_PROCESS_SIZE + 1);
    let streaming_hash = pcm_hash(&output);
    owner.flush(&mut ending).expect("one bounded flush");
    assert_eq!(
        owner.flush(&mut ending),
        Err(4),
        "terminal guard before a second FFI call"
    );
    for channel in 0..PROBE_CHANNELS {
        output[channel].extend_from_slice(&ending[channel]);
        assert_eq!(output[channel].len(), raw_frames);
        assert!(output[channel].iter().all(|n| n.is_finite()));
    }
    drop(owner);
    emit(serde_json::json!({
        "stage":"signalsmith_complete_raw_capture","caller":caller.0,"q":1.0,"p":pitch,
        "canonical_frames":frames,"canonical_pcm_sha256":pcm_hash(input),
        "P":0,"streaming_input_real_frames_once":frames,"streaming_zero_input_frames_Li":INPUT_LATENCY,
        "streaming_output_frames":total,"native_process_blocks":blocks,
        "streaming_output_pcm_sha256":streaming_hash,"outer_caller_calls":calls,
        "one_flush_output_frames_Lo_plus_2W":FLUSH,"flush_playbackRate":1.0,
        "complete_flush_output_pcm_sha256":pcm_hash(&ending),
        "flush_internal_zero_input_frames_F_minus_I":FLUSH-INTERVAL,
        "flush_last_interval_window_taper_reverse_subtraction_reset_frames":INTERVAL,
        "terminal_fold_starts_at_raw_frame":total+FLUSH-INTERVAL,
        "entire_raw_frames":raw_frames,"entire_raw_pcm_sha256":pcm_hash(&output),
        "D_before_signal":DELAY,"nominal_raw_window":[DELAY,DELAY+frames],
        "extra_suffix_frames_2W":2*WINDOW,
        "ending_contract":"one fixed finite streaming+flush capture; not EOF or internal tail exhaustion",
        "all_returned_samples_retained":true,"gain_crop_fit_fake_output_padding":false,
        "owner_retired_before_scoring":true,
        "actual_Raw_channel_capacities":output.iter().map(Vec::capacity).collect::<Vec<_>>(),
        "actual_ending_channel_capacities":ending.iter().map(Vec::capacity).collect::<Vec<_>>(),
        "actual_process_block_capacity":blocks.capacity(),
        "actual_current_Raw_capacity_and_headers_bytes":pcm_bytes(&output),
        "actual_ending_capacity_and_headers_bytes":pcm_bytes(&ending),
        "actual_previous_Raw_capacity_and_headers_bytes":previous.map_or(0,pcm_bytes),
        "current_and_previous_Raw_capacity_checked_before_scoring":true
    }));
    Capture {
        raw: output,
        delay: DELAY,
        process_block_capacity: blocks.capacity(),
    }
}

#[test]
fn signalsmith_owner_rejects_invalid_and_terminal_calls() {
    assert!(matches!(Probe::new(f64::NAN), Err(1)));
    let mut owner = Probe::new(1.0).unwrap();
    let mut input = [[0.0; MAX_PROCESS_SIZE]; PROBE_CHANNELS];
    let mut output = [[0.0; MAX_PROCESS_SIZE]; PROBE_CHANNELS];
    assert_eq!(owner.process(&input, &mut output, 0), Err(1));
    assert_eq!(
        owner.process(&input, &mut output, MAX_PROCESS_SIZE + 1),
        Err(1)
    );
    input[0][0] = f32::NAN;
    assert_eq!(owner.process(&input, &mut output, 1), Err(1));
    assert_eq!(owner.processed, 0);
    input[0][0] = 0.0;
    owner.process(&input, &mut output, 1).unwrap();
    let mut ending = vec![vec![0.0; FLUSH]; PROBE_CHANNELS];
    owner.flush(&mut ending).unwrap();
    assert_eq!(owner.process(&input, &mut output, 1), Err(4));
    assert_eq!(owner.flush(&mut ending), Err(4));
}
