//! Offline Key Lock measurement. Run with `uv run cargo run --release --manifest-path
//! rust/Cargo.toml -p flitzis-looper --example key_lock_latency_probe > probe.csv`.
//! On Windows, put the documented Rubber Band runtime directory on PATH first.
//! This imports the production implementation directly; it never starts an audio device.
//! Rust allocation counts exclude allocations inside the native Rubber Band/FFT libraries.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::time::Instant;

#[path = "../src/audio_engine/constants.rs"]
pub mod constants;
#[path = "../src/audio_engine/key_lock_preparation.rs"]
mod key_lock_preparation;
#[path = "../src/audio_engine/rubberband_backend.rs"]
mod rubberband_backend;
mod audio_engine {
    pub(crate) use crate::constants;
    pub(crate) use crate::key_lock_preparation;
    pub(crate) use crate::rubberband_backend;
}
#[path = "../src/audio_engine/stretch_processor.rs"]
mod stretch_processor;

use rubberband_backend::RubberBandLiveShifter;
use stretch_processor::{DEFAULT_BLOCK_SAMPLES, StretchProcessor};

// Cargo applies the package's link-lib instruction to its cdylib target. The standalone
// path-imported probe also needs the native library; build.rs supplies its search directory.
#[link(name = "rubberband")]
unsafe extern "C" {}

struct CountingAllocator;
thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<u64> = const { Cell::new(0) };
    static ALLOCATION_BYTES: Cell<u64> = const { Cell::new(0) };
}

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record_allocation(layout.size());
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record_allocation(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record_allocation(size);
        unsafe { System.realloc(pointer, layout, size) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn record_allocation(size: usize) {
    if COUNTING.try_with(Cell::get).unwrap_or(false) {
        ALLOCATIONS.with(|count| count.set(count.get() + 1));
        ALLOCATION_BYTES.with(|count| count.set(count.get() + size as u64));
    }
}

struct Case<'a> {
    rate: u32,
    ratio: f32,
    pattern: &'a str,
    trial: usize,
}

impl Case<'_> {
    fn emit(&self, kind: &str, metric: &str, value: impl std::fmt::Display, unit: &str) {
        println!(
            "{kind},{},{},{},{},{metric},{value},{unit}",
            self.rate, self.ratio, self.pattern, self.trial
        );
    }

    fn measure<T>(&self, operation: &str, action: impl FnOnce() -> T) -> T {
        ALLOCATIONS.set(0);
        ALLOCATION_BYTES.set(0);
        COUNTING.set(true);
        let start = Instant::now();
        let result = action();
        let elapsed = start.elapsed().as_nanos();
        COUNTING.set(false);
        self.emit("timing", operation, elapsed, "ns");
        self.emit("rust_allocations", operation, ALLOCATIONS.get(), "count");
        self.emit(
            "rust_allocations",
            operation,
            ALLOCATION_BYTES.get(),
            "bytes",
        );
        result
    }
}

fn response_metrics(case: &Case<'_>, kind: &str, output: &[f32], reference: usize) {
    assert!(output.iter().all(|sample| sample.is_finite()));
    let (peak_frame, peak) = output
        .iter()
        .enumerate()
        .max_by(|(_, left), (_, right)| left.abs().total_cmp(&right.abs()))
        .map(|(index, value)| (index, value.abs()))
        .unwrap();
    case.emit(kind, "reference_frame", reference, "frames");
    case.emit(kind, "peak_amplitude", peak, "linear");
    if peak <= 1.0e-7 {
        case.emit(kind, "detected", 0, "bool");
        return;
    }
    case.emit(kind, "detected", 1, "bool");
    let onset = output
        .iter()
        .position(|value| value.abs() > 1.0e-7)
        .unwrap();
    let significant = output
        .iter()
        .position(|value| value.abs() >= peak * 0.01)
        .unwrap();
    for (metric, frame) in [
        ("onset_frame", onset),
        ("one_percent_onset_frame", significant),
        ("peak_frame", peak_frame),
    ] {
        case.emit(kind, metric, frame, "frames");
        case.emit(
            kind,
            &format!("{metric}_minus_reference"),
            frame as i64 - reference as i64,
            "frames",
        );
    }
}

fn new_processor(
    rate: u32,
) -> (
    StretchProcessor,
    key_lock_preparation::KeyLockPreparationWorker,
) {
    let (mut lanes, worker) =
        key_lock_preparation::create_key_lock_preparation(2, rate, 1).unwrap();
    (
        StretchProcessor::with_preparation_lane(2, lanes.remove(0)),
        worker,
    )
}

fn measure_preparation(rate: u32, ratio: f32, trials: usize) {
    for trial in 0..trials {
        let case = Case {
            rate,
            ratio,
            pattern: "512",
            trial,
        };
        let mut native = case.measure("native_construct", || {
            RubberBandLiveShifter::new(rate, 2).unwrap()
        });
        case.measure("native_first_pitch_update", || {
            native.set_pitch_scale(1.0 / f64::from(ratio)).unwrap()
        });
        let block = native.block_size();
        let input = vec![vec![0.0; block]; 2];
        let mut output = vec![vec![0.0; block]; 2];
        case.measure("native_first_shift", || {
            native.shift(&input, &mut output).unwrap()
        });
        case.measure("native_reset_after_shift", || {
            native.reset_for_preparation()
        });
        case.measure("native_same_pitch_update_after_reset", || {
            native.set_pitch_scale(1.0 / f64::from(ratio)).unwrap()
        });
        case.measure("native_first_shift_after_reset", || {
            native.shift(&input, &mut output).unwrap()
        });
        case.measure("native_changed_pitch_update", || {
            native
                .set_pitch_scale(if ratio > 1.0 { 1.25 } else { 0.8 })
                .unwrap()
        });
        case.measure("native_first_shift_after_changed_pitch", || {
            native.shift(&input, &mut output).unwrap()
        });
        case.measure("native_prepare_for_reuse", || {
            native.prepare_for_reuse().unwrap()
        });
        case.measure("native_warmed_pitch_update", || {
            native.set_pitch_scale(1.0 / f64::from(ratio)).unwrap()
        });
        case.measure("native_warmed_first_shift", || {
            native.shift(&input, &mut output).unwrap()
        });
        let (mut adapter, _worker) = case.measure("adapter_construct", || new_processor(rate));
        for channel in adapter.resampled_buffers_mut(512) {
            channel.fill(0.25);
        }
        case.measure("adapter_first_activate_and_process", || {
            adapter.process_resampled(512, f64::from(ratio), true)
        });
        case.measure("adapter_reset_after_process", || adapter.reset());
        for channel in adapter.resampled_buffers_mut(512) {
            channel.fill(0.25);
        }
        case.measure("adapter_activate_after_reset", || {
            adapter.process_resampled(512, f64::from(ratio), true)
        });
        case.measure("adapter_warm_process", || {
            adapter.process_resampled(512, f64::from(ratio), true)
        });
    }
}

fn measure_native_response(rate: u32, ratio: f32, marker: usize, warmed: bool) {
    let case = Case {
        rate,
        ratio,
        pattern: "native",
        trial: marker,
    };
    let mut native = RubberBandLiveShifter::new(rate, 2).unwrap();
    let kind = if warmed { "native_warmed" } else { "native" };
    if warmed {
        native.prepare_for_reuse().unwrap();
    }
    case.emit(kind, "delay_before_pitch", native.start_delay(), "frames");
    native.set_pitch_scale(1.0 / f64::from(ratio)).unwrap();
    let block = native.block_size();
    let delay = native.start_delay();
    case.emit(kind, "block_size", block, "frames");
    case.emit(kind, "delay_after_pitch", delay, "frames");
    case.emit(
        kind,
        "delay_after_pitch_ms",
        delay as f64 * 1000.0 / rate as f64,
        "ms",
    );
    let mut input = vec![vec![0.0; block]; 2];
    let mut output = vec![vec![0.0; block]; 2];
    let mut rendered = Vec::new();
    for offset in (0..marker + delay + block * 8).step_by(block) {
        for channel in &mut input {
            channel.fill(0.0);
            if (offset..offset + block).contains(&marker) {
                channel[marker - offset] = 1.0;
            }
        }
        native.shift(&input, &mut output).unwrap();
        rendered.extend_from_slice(&output[0]);
    }
    response_metrics(
        &case,
        if warmed {
            "native_response_warmed"
        } else {
            "native_response"
        },
        &rendered,
        marker,
    );
}

fn adapter_response(
    rate: u32,
    ratio: f32,
    pattern: &[usize],
    marker: usize,
    key_lock: bool,
) -> Vec<f32> {
    let (mut adapter, _worker) = new_processor(rate);
    let total_frames = marker + rate as usize / 3;
    let source_rate = f64::from(ratio);
    // One immutable source-domain impulse is linearly sampled at each absolute output frame.
    // Callback boundaries do not relocate its marker or choose its interpolation endpoints.
    let source_frames = (total_frames as f64 * source_rate).ceil() as usize + 2;
    let mut source = vec![0.0; source_frames];
    source[(marker as f64 * source_rate).round() as usize] = 1.0;
    let mut offset = 0;
    let mut index = 0;
    let mut rendered = Vec::new();
    while offset < total_frames {
        let output_samples = pattern[index % pattern.len()].min(total_frames - offset);
        for channel in adapter.resampled_buffers_mut(output_samples) {
            for (index, sample) in channel[..output_samples].iter_mut().enumerate() {
                let position = (offset + index) as f64 * source_rate;
                let source_index = position.floor() as usize;
                let fraction = (position - source_index as f64) as f32;
                *sample = source[source_index]
                    + (source[source_index + 1] - source[source_index]) * fraction;
            }
        }
        adapter.process_resampled(output_samples, f64::from(ratio), key_lock);
        rendered.extend_from_slice(&adapter.output_buffers()[0][..output_samples]);
        offset += output_samples;
        index += 1;
    }
    rendered
}

/// Independent counters for fixed-lead FIFO scheduling. This is a model, not access to hidden
/// production FIFO state. Impulse delay validates its observable result. Baseline CSV collected
/// before the fixed-lead change used this same counter model with initial output occupancy zero.
fn measure_adapter_fifo(case: &Case<'_>, pattern: &[usize], block: usize) {
    let mut input = 0;
    let mut output = block.saturating_sub(1);
    let mut total_missing = 0;
    let mut missing_callbacks = 0;
    let mut last_missing_frame = 0;
    let mut elapsed = 0;
    let mut max_input = 0;
    let mut max_output = 0;
    let mut max_shifts = 0;
    let shift_bound = DEFAULT_BLOCK_SAMPLES / block + 2;
    for index in 0..2048 {
        let frames = pattern[index % pattern.len()];
        input += frames;
        max_input = max_input.max(input);
        let shifts = (input / block).min(shift_bound);
        input -= shifts * block;
        output += shifts * block;
        max_output = max_output.max(output);
        max_shifts = max_shifts.max(shifts);
        let read = frames.min(output);
        output -= read;
        let missing = frames - read;
        if missing > 0 {
            missing_callbacks += 1;
            total_missing += missing;
            last_missing_frame = elapsed + frames;
        }
        elapsed += frames;
        assert!(input <= block + DEFAULT_BLOCK_SAMPLES);
        assert!(output <= block * 2 + DEFAULT_BLOCK_SAMPLES);
    }
    for (metric, value, unit) in [
        ("initial_output_lead", block.saturating_sub(1), "frames"),
        ("underflow_frames", total_missing, "frames"),
        ("underflow_callbacks", missing_callbacks, "count"),
        ("last_underflow_output_frame", last_missing_frame, "frames"),
        ("max_input_fifo_before_shift", max_input, "frames"),
        ("max_output_fifo_before_pop", max_output, "frames"),
        ("max_shifts_per_callback", max_shifts, "count"),
        ("shift_bound", shift_bound, "count"),
        ("observed_frames", elapsed, "frames"),
    ] {
        case.emit("adapter_fifo_model", metric, value, unit);
    }
}

#[cfg(windows)]
fn process_memory() -> Option<(usize, usize)> {
    // PROCESS_MEMORY_COUNTERS_EX is two DWORDs followed by nine SIZE_T fields.
    // https://learn.microsoft.com/windows/win32/api/psapi/ns-psapi-process_memory_counters_ex
    #[repr(C)]
    struct MemoryCounters {
        size: u32,
        page_faults: u32,
        counters: [usize; 9],
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn K32GetProcessMemoryInfo(
            process: *mut std::ffi::c_void,
            counters: *mut MemoryCounters,
            size: u32,
        ) -> i32;
    }
    let size = std::mem::size_of::<MemoryCounters>() as u32;
    let mut counters = MemoryCounters {
        size,
        page_faults: 0,
        counters: [0; 9],
    };
    // The -1 pseudo-handle names only this probe's own process and needs no open/close call.
    let success =
        unsafe { K32GetProcessMemoryInfo(-1isize as *mut std::ffi::c_void, &mut counters, size) };
    (success != 0).then_some((counters.counters[1], counters.counters[8]))
}

#[cfg(not(windows))]
fn process_memory() -> Option<(usize, usize)> {
    None
}

fn measure_pool(rate: u32, voices: usize) {
    let case = Case {
        rate,
        ratio: 1.0,
        pattern: "pool",
        trial: 0,
    };
    case.emit("pool", "voices", voices, "count");
    case.emit("pool", "native_handles", voices * 2, "count");
    let before = process_memory();
    let (lanes, worker) = case.measure("pool_construct_and_warm", || {
        key_lock_preparation::create_key_lock_preparation(2, rate, voices).unwrap()
    });
    if let (Some(before), Some(after)) = (before, process_memory()) {
        for (metric, old, new) in [
            ("working_set", before.0, after.0),
            ("private_committed", before.1, after.1),
        ] {
            case.emit("pool_memory", &format!("{metric}_before"), old, "bytes");
            case.emit("pool_memory", &format!("{metric}_after"), new, "bytes");
            case.emit(
                "pool_memory",
                &format!("{metric}_delta"),
                new as i64 - old as i64,
                "bytes",
            );
            case.emit(
                "pool_memory",
                &format!("{metric}_delta_per_handle"),
                (new as f64 - old as f64) / (voices * 2) as f64,
                "bytes",
            );
        }
    }
    drop(lanes);
    drop(worker);
}

fn main() {
    println!("kind,sample_rate_hz,tempo_ratio,callback_pattern,trial,metric,value,unit");
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.first().is_some_and(|argument| argument == "pool") {
        let rate = args
            .get(1)
            .expect("pool needs rate")
            .parse()
            .expect("rate must be an integer");
        let voices = args
            .get(2)
            .expect("pool needs voices")
            .parse()
            .expect("voices must be an integer");
        measure_pool(rate, voices);
        return;
    }
    let trials = args.first().map_or(24, |value| {
        value.parse::<usize>().expect("trials must be an integer")
    });
    let patterns: &[(&str, &[usize])] = &[
        ("64", &[64]),
        ("128", &[128]),
        ("256", &[256]),
        ("512", &[512]),
        ("64+96+257+512+31+1", &[64, 96, 257, 512, 31, 1]),
    ];
    for rate in [44_100, 48_000, 96_000] {
        for ratio in [0.5, 0.75, 1.0, 1.5, 2.0] {
            measure_preparation(rate, ratio, trials);
            let native = RubberBandLiveShifter::new(rate, 2).unwrap();
            for marker in [0, 8192] {
                measure_native_response(rate, ratio, marker, false);
                measure_native_response(rate, ratio, marker, true);
                for (name, pattern) in patterns {
                    let case = Case {
                        rate,
                        ratio,
                        pattern: name,
                        trial: marker,
                    };
                    let baseline = adapter_response(rate, ratio, pattern, marker, false);
                    let reference = baseline
                        .iter()
                        .enumerate()
                        .max_by(|(_, left), (_, right)| left.abs().total_cmp(&right.abs()))
                        .unwrap()
                        .0;
                    let locked = adapter_response(rate, ratio, pattern, marker, true);
                    response_metrics(&case, "adapter_response", &locked, reference);
                    if marker == 0 && ratio != 1.0 {
                        measure_adapter_fifo(&case, pattern, native.block_size());
                    }
                }
            }
        }
    }
}
