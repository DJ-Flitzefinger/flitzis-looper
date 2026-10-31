//! Independent offline BeatThis frontend/chunking/minimal postprocessing trial.
//! No application dispatch, realtime work, Python or LibTorch is involved.

use super::beat_inference::{BINS, FRAMES, Logits, checkpoint};
use libloading::Library;
use rustfft::{FftPlanner, num_complex::Complex};
use sha2::{Digest, Sha256};
use std::{
    ffi::{CStr, c_char, c_void},
    fs::File,
    io::Read,
    path::Path,
};

pub const RATE: u32 = 22050;
pub const HOP: usize = 441;
pub const FFT: usize = 1024;
pub const BORDER: usize = 6;
pub const MAX_AUDIO_BYTES: usize = 256 * 1024 * 1024;

type Create = unsafe extern "C" fn(
    f64,
    f64,
    u32,
    *mut *const c_char,
    *const c_void,
    *const c_void,
    *const c_void,
) -> *mut c_void;
type Process = unsafe extern "C" fn(
    *mut c_void,
    *const c_void,
    usize,
    *mut usize,
    *mut c_void,
    usize,
    *mut usize,
) -> *const c_char;
type Delete = unsafe extern "C" fn(*mut c_void);
type Version = unsafe extern "C" fn() -> *const c_char;

/// Explicit caller-owned, digest-checked libsoxr-0.1.3, never a Python extension.
pub struct Soxr {
    library: Library,
    // Keep the verified file open through all native users (including DLL unload).
    _file: File,
}

impl Soxr {
    /// Loading native code is restricted to the admitted developer trial caller.
    pub fn load(path: &Path, expected: &str) -> Result<Self, String> {
        if !path.is_absolute() {
            return Err("libsoxr path must be absolute".into());
        }
        let mut file = File::open(path).map_err(|e| e.to_string())?;
        if file.metadata().map_err(|e| e.to_string())?.len() > 32 * 1024 * 1024 {
            return Err("libsoxr exceeds trial bound".into());
        }
        let mut digest = Sha256::new();
        let mut buffer = [0u8; 65536];
        let mut bytes = 0usize;
        loop {
            let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            bytes += n;
            if bytes > 32 * 1024 * 1024 {
                return Err("libsoxr changed beyond trial byte bound".into());
            }
            digest.update(&buffer[..n]);
        }
        if expected.len() != 64 || format!("{:x}", digest.finalize()) != expected {
            return Err("libsoxr digest mismatch".into());
        }
        // SAFETY: caller explicitly supplies a trusted native library and digest;
        // the retained library owns every resolved symbol for the entire call.
        // The caller must keep this owned trial path immutable. Hashing the file
        // does not attest an OS-loaded image or a complete product producer.
        let library = unsafe { Library::new(path) }.map_err(|e| e.to_string())?;
        let version = unsafe {
            let get = library
                .get::<Version>(b"soxr_version\0")
                .map_err(|e| e.to_string())?;
            CStr::from_ptr(get())
                .to_str()
                .map_err(|e| e.to_string())?
                .to_owned()
        };
        if version != "libsoxr-0.1.3" {
            return Err(format!("unsupported {version}"));
        }
        Ok(Self {
            library,
            _file: file,
        })
    }

    /// Original full-buffer HQ resample, using the original wrapper's block size
    /// and flushing the real tail. Default C specs are FLOAT32/HQ/one thread.
    pub fn resample(
        &self,
        signal: &[f32],
        rate: u32,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<f32>, String> {
        validate_signal(signal, rate)?;
        checkpoint(cancelled)?;
        if rate == RATE {
            return Ok(signal.to_vec());
        }
        let capacity = ((signal.len() as u64 * RATE as u64) / rate as u64 + 1) as usize;
        if capacity > MAX_AUDIO_BYTES / 4 {
            return Err("resampled audio exceeds trial bound".into());
        }
        let mut output = vec![0.0; capacity];
        // SAFETY: libsoxr's documented ABI; mono f32 slices remain allocated,
        // counters bound every pointer and the handle is deleted on every result.
        unsafe {
            let create = self
                .library
                .get::<Create>(b"soxr_create\0")
                .map_err(|e| e.to_string())?;
            let process = self
                .library
                .get::<Process>(b"soxr_process\0")
                .map_err(|e| e.to_string())?;
            let delete = self
                .library
                .get::<Delete>(b"soxr_delete\0")
                .map_err(|e| e.to_string())?;
            let mut error = std::ptr::null();
            let handle = create(
                rate as f64,
                RATE as f64,
                1,
                &mut error,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
            );
            if handle.is_null() || !error.is_null() {
                if !handle.is_null() {
                    delete(handle);
                }
                return Err(soxr_error(error));
            }
            let result = (|| {
                let block = (48000.0 * rate as f64 / RATE as f64).max(1000.0) as usize;
                let mut written = 0;
                for input in signal.chunks(block) {
                    checkpoint(cancelled)?;
                    let mut used = 0;
                    let mut done = 0;
                    let error = process(
                        handle,
                        input.as_ptr().cast(),
                        input.len(),
                        &mut used,
                        output[written..].as_mut_ptr().cast(),
                        capacity - written,
                        &mut done,
                    );
                    if !error.is_null() {
                        return Err(soxr_error(error));
                    }
                    if used != input.len() || done > capacity - written {
                        return Err("incomplete libsoxr input".into());
                    }
                    written += done;
                }
                checkpoint(cancelled)?;
                let mut done = 0;
                let error = process(
                    handle,
                    std::ptr::null(),
                    0,
                    std::ptr::null_mut(),
                    output[written..].as_mut_ptr().cast(),
                    capacity - written,
                    &mut done,
                );
                if !error.is_null() {
                    return Err(soxr_error(error));
                }
                if done > capacity - written {
                    return Err("invalid libsoxr tail".into());
                }
                written += done;
                output.truncate(written);
                checkpoint(cancelled)?;
                Ok(output)
            })();
            delete(handle);
            result
        }
    }
}

fn soxr_error(error: *const c_char) -> String {
    if error.is_null() {
        "libsoxr creation failed".into()
    } else {
        unsafe { CStr::from_ptr(error) }
            .to_string_lossy()
            .into_owned()
    }
}

fn validate_signal(signal: &[f32], rate: u32) -> Result<(), String> {
    if rate == 0
        || rate > 384000
        || signal.is_empty()
        || signal.len() > MAX_AUDIO_BYTES / 4
        || !signal.iter().all(|x| x.is_finite())
    {
        return Err("invalid complete mono PCM or trial byte bound".into());
    }
    Ok(())
}

fn linspace(start: f32, end: f32, count: usize, i: usize) -> f32 {
    let step = (end - start) / (count - 1) as f32;
    if i < count / 2 {
        start + step * i as f32
    } else {
        end - step * (count - i - 1) as f32
    }
}

fn mel(hz: f64) -> f64 {
    if hz < 1000.0 {
        hz / (200.0 / 3.0)
    } else {
        15.0 + (hz / 1000.0).ln() / (6.4f64.ln() / 27.0)
    }
}

fn hz(m: f32) -> f32 {
    if m < 15.0 {
        (200.0f32 / 3.0) * m
    } else {
        1000.0 * ((6.4f64.ln() / 27.0) as f32 * (m - 15.0)).exp()
    }
}

/// Centered reflect512 periodic Hann, magnitude /sqrt(1024), Slaney frequency
/// scale with norm=None (no area normalization), then log1p(1000*mel).
pub fn log_mel(signal: &[f32], cancelled: &dyn Fn() -> bool) -> Result<Vec<f32>, String> {
    validate_signal(signal, RATE)?;
    if signal.len() <= FFT / 2 {
        return Err("audio too short for reference reflect padding".into());
    }
    checkpoint(cancelled)?;
    let frames = signal.len() / HOP + 1;
    let mut output = vec![0.0; frames * BINS];
    let window: Vec<_> = (0..FFT)
        .map(|i| 0.5 - 0.5 * ((2.0 * std::f32::consts::PI / FFT as f32) * i as f32).cos())
        .collect();
    let points: Vec<_> = (0..BINS + 2)
        .map(|i| hz(linspace(mel(30.0) as f32, mel(11000.0) as f32, BINS + 2, i)))
        .collect();
    let mut filters = vec![0.0; (FFT / 2 + 1) * BINS];
    for k in 0..=FFT / 2 {
        let frequency = linspace(0.0, (RATE / 2) as f32, FFT / 2 + 1, k);
        for b in 0..BINS {
            filters[k * BINS + b] = ((frequency - points[b]) / (points[b + 1] - points[b]))
                .min((points[b + 2] - frequency) / (points[b + 2] - points[b + 1]))
                .max(0.0);
        }
    }
    let fft = FftPlanner::<f32>::new().plan_fft_forward(FFT);
    let mut data = vec![Complex::new(0.0, 0.0); FFT];
    let mut scratch = vec![Complex::new(0.0, 0.0); fft.get_inplace_scratch_len()];
    for frame in 0..frames {
        checkpoint(cancelled)?;
        for i in 0..FFT {
            let p = (frame * HOP + i) as isize - (FFT / 2) as isize;
            let index = if p < 0 {
                -p
            } else if p >= signal.len() as isize {
                2 * signal.len() as isize - 2 - p
            } else {
                p
            } as usize;
            data[i] = Complex::new(signal[index] * window[i], 0.0);
        }
        fft.process_with_scratch(&mut data, &mut scratch);
        let row = &mut output[frame * BINS..(frame + 1) * BINS];
        for (k, z) in data[..=FFT / 2].iter().enumerate() {
            let magnitude = (z.re * z.re + z.im * z.im).sqrt() / 32.0;
            for b in 0..BINS {
                row[b] += magnitude * filters[k * BINS + b];
            }
        }
        for x in row {
            *x = (*x * 1000.0).ln_1p();
        }
    }
    if !output.iter().all(|x| x.is_finite()) {
        return Err("nonfinite native log-mel".into());
    }
    Ok(output)
}

/// Exact upstream start positions, including the shifted and duplicated tail.
pub fn chunk_starts(frames: usize) -> Result<Vec<isize>, String> {
    if frames == 0 || frames > MAX_AUDIO_BYTES / 4 / HOP + 1 {
        return Err("invalid spectrogram length".into());
    }
    let mut starts: Vec<_> = (0..frames)
        .step_by(FRAMES - 2 * BORDER)
        .map(|s| s as isize - BORDER as isize)
        .collect();
    if frames > FRAMES - 2 * BORDER {
        *starts.last_mut().unwrap() = frames as isize - (FRAMES - BORDER) as isize;
    }
    Ok(starts)
}

/// One chunk at a time; short pieces really use T+12 model frames. Retaining only
/// the earliest accepted output implements keep_first without keeping all chunks.
pub fn predict(
    spect: &[f32],
    mut run: impl FnMut(&[f32]) -> Result<Logits, String>,
    cancelled: &dyn Fn() -> bool,
) -> Result<Logits, String> {
    if spect.len() % BINS != 0 || !spect.iter().all(|x| x.is_finite()) {
        return Err("invalid spectrogram".into());
    }
    let frames = spect.len() / BINS;
    let starts = chunk_starts(frames)?;
    let mut output = Logits {
        beat: vec![-1000.0; frames],
        downbeat: vec![-1000.0; frames],
    };
    let mut assigned = 0;
    for start in starts {
        checkpoint(cancelled)?;
        let left = (-start).max(0) as usize;
        let first = start.max(0) as usize;
        let end = (start + FRAMES as isize).min(frames as isize) as usize;
        let right = (start + FRAMES as isize - frames as isize).clamp(0, BORDER as isize) as usize;
        let length = left + end - first + right;
        let mut chunk = vec![0.0; length * BINS];
        chunk[left * BINS..(left + end - first) * BINS]
            .copy_from_slice(&spect[first * BINS..end * BINS]);
        let logits = run(&chunk)?;
        if logits.beat.len() != length
            || logits.downbeat.len() != length
            || !logits
                .beat
                .iter()
                .chain(&logits.downbeat)
                .all(|x| x.is_finite())
        {
            return Err("invalid chunk logits".into());
        }
        let begin = (start + BORDER as isize) as usize;
        let until = (begin + length - 2 * BORDER).min(frames);
        for i in assigned.max(begin)..until {
            output.beat[i] = logits.beat[i - begin + BORDER];
            output.downbeat[i] = logits.downbeat[i - begin + BORDER];
        }
        assigned = assigned.max(until);
    }
    checkpoint(cancelled)?;
    if assigned != frames {
        return Err("incomplete native aggregation".into());
    }
    Ok(output)
}

fn peaks(logits: &[f32]) -> Vec<f64> {
    let mut result = Vec::new();
    let mut count = 0;
    let mut mean = 0.0;
    for i in 0..logits.len() {
        if logits[i] <= 0.0
            || logits[i.saturating_sub(3)..(i + 4).min(logits.len())]
                .iter()
                .any(|x| *x > logits[i])
        {
            continue;
        }
        if count != 0 && i as f64 - mean <= 1.0 {
            count += 1;
            mean += (i as f64 - mean) / count as f64;
        } else {
            if count != 0 {
                result.push(mean / 50.0);
            }
            mean = i as f64;
            count = 1;
        }
    }
    if count != 0 {
        result.push(mean / 50.0);
    }
    result
}

/// Minimal postprocessor; duration exclusion follows downbeat-to-beat snapping.
pub fn postprocess(logits: &Logits, duration: f64) -> Result<(Vec<f64>, Vec<f64>), String> {
    if !duration.is_finite()
        || duration <= 0.0
        || logits.beat.is_empty()
        || logits.beat.len() != logits.downbeat.len()
        || !logits
            .beat
            .iter()
            .chain(&logits.downbeat)
            .all(|x| x.is_finite())
    {
        return Err("invalid postprocessor input".into());
    }
    let mut beats = peaks(&logits.beat);
    let mut downbeats = peaks(&logits.downbeat);
    if !beats.is_empty() {
        for d in &mut downbeats {
            *d = *beats
                .iter()
                .min_by(|a, b| (**a - *d).abs().total_cmp(&(**b - *d).abs()))
                .unwrap();
        }
    }
    downbeats.sort_by(f64::total_cmp);
    downbeats.dedup();
    beats.retain(|t| *t >= 0.0 && *t < duration);
    downbeats.retain(|t| *t >= 0.0 && *t < duration);
    Ok((beats, downbeats))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_chunk_boundaries_and_short_attention_extent() {
        for (t, expected) in [
            (2, vec![-6]),
            (1488, vec![-6]),
            (1489, vec![-6, -5]),
            (2976, vec![-6, 1482]),
            (2977, vec![-6, 1482, 1483]),
        ] {
            assert_eq!(chunk_starts(t).unwrap(), expected);
            let spect: Vec<_> = (0..t * BINS).map(|i| (i / BINS) as f32).collect();
            let mut lengths = Vec::new();
            let actual = predict(
                &spect,
                |chunk| {
                    lengths.push(chunk.len() / BINS);
                    let values: Vec<_> = chunk.chunks_exact(BINS).map(|r| r[0]).collect();
                    Ok(Logits {
                        beat: values.clone(),
                        downbeat: values,
                    })
                },
                &|| false,
            )
            .unwrap();
            assert_eq!(actual.beat, (0..t).map(|i| i as f32).collect::<Vec<_>>());
            assert_eq!(lengths[0], (t + 12).min(1500));
        }
    }
    #[test]
    fn keep_first_and_postprocess_running_mean_ties_duration() {
        let mut n = 0.0;
        let output = predict(
            &vec![0.0; 1489 * BINS],
            |chunk| {
                n += 1.0;
                Ok(Logits {
                    beat: vec![n; chunk.len() / BINS],
                    downbeat: vec![n; chunk.len() / BINS],
                })
            },
            &|| false,
        )
        .unwrap();
        assert_eq!(&output.beat[..1488], &vec![1.0; 1488]);
        assert_eq!(output.beat[1488], 2.0);
        assert_eq!(peaks(&[1.0, 1.0, 1.0]), vec![0.01, 0.04]);
        let logits = Logits {
            beat: vec![1.0, -1.0, -1.0, -1.0, 1.0],
            downbeat: vec![-1.0, -1.0, 1.0, -1.0, -1.0],
        };
        assert_eq!(postprocess(&logits, 0.08).unwrap(), (vec![0.0], vec![0.0]));
    }
    #[test]
    fn invalid_and_cancelled_frontend_never_runs_model() {
        assert!(log_mel(&vec![0.0; 512], &|| false).is_err());
        assert!(
            log_mel(&vec![0.0; 513], &|| true)
                .unwrap_err()
                .contains("cancelled")
        );
        assert!(
            predict(
                &vec![f32::NAN; BINS],
                |_| panic!("invalid model run"),
                &|| false
            )
            .is_err()
        );
        assert!(
            log_mel(&vec![0.0; 513], &|| false)
                .unwrap()
                .iter()
                .all(|x| *x == 0.0)
        );
    }
}
