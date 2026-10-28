//! Optional BPM-only preparation on the same protected complete playback PCM.
use super::{
    PcmError, check_cancelled, default::normal_mean, fft, resample_mono_cancellable, validate_rate,
};
use crate::messages::SampleBuffer;
use sha2::{Digest, Sha256};

const FIXED: usize = 1024 * 1024;
const CHUNK: usize = 4096;

pub(in crate::audio_engine) struct QmPcm {
    pub samples: Vec<f64>,
    pub loaded_interleaved_sha256: String,
    pub normal_mono_sha256: String,
    pub analyzer_sha256: String,
    pub resampler: &'static str,
    pub qm_working_bytes: usize,
    pub preparation_peak_bytes: usize,
}

fn sum(values: &[usize]) -> Result<usize, PcmError> {
    values
        .iter()
        .try_fold(0_usize, |n, value| n.checked_add(*value))
        .ok_or(PcmError::Limit("QM preparation overflow"))
}
fn bytes<T>(n: usize) -> Result<usize, PcmError> {
    n.checked_mul(size_of::<T>())
        .ok_or(PcmError::Limit("QM preparation extent"))
}
fn admit(values: &[usize], available: usize) -> Result<usize, PcmError> {
    let peak = sum(&[sum(values)?, FIXED])?;
    if peak > available {
        return Err(PcmError::Limit("optional complete QM preparation"));
    }
    Ok(peak)
}
fn reserve<T>(n: usize) -> Result<Vec<T>, PcmError> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(n)
        .map_err(|_| PcmError::Limit("QM allocation unavailable"))?;
    if values.capacity() > n {
        return Err(PcmError::Limit("QM unexpected allocation capacity"));
    }
    Ok(values)
}

/// `available` excludes every caller-owned source, raw capture and base method.
/// The normal scientific path keeps its existing CQT-inclusive admission.
pub(in crate::audio_engine) fn prepare_qm_input(
    sample: &SampleBuffer,
    rate: u32,
    verified_nn_mono: &str,
    available: usize,
    cancelled: &impl Fn() -> bool,
) -> Result<QmPcm, PcmError> {
    check_cancelled(cancelled)?;
    validate_rate(rate)?;
    if !matches!(sample.channels, 1 | 2)
        || sample.samples.is_empty()
        || !sample.samples.len().is_multiple_of(sample.channels)
        || sample.resident_start() != 0
        || sample.resident_end() != sample.frame_count()
    {
        return Err(PcmError::InvalidInput(
            "QM requires complete mono or stereo playback",
        ));
    }
    let frames = sample.samples.len() / sample.channels;
    let output = usize::try_from((frames as u128 * 44_100).div_ceil(u128::from(rate)))
        .map_err(|_| PcmError::Limit("QM output extent"))?;
    let (input_fft, output_fft) = if rate == 44_100 {
        (0, 0)
    } else {
        fft::fft_dimensions(rate, 44_100)
    };
    let fft_bytes = sum(&[input_fft, output_fft])?
        .checked_mul(64 * 4)
        .ok_or(PcmError::Limit("QM FFT extent"))?;
    let converted_capacity = sum(&[
        output,
        output_fft
            .checked_mul(2)
            .ok_or(PcmError::Limit("QM delayed extent"))?,
    ])?;
    let converted_bytes = bytes::<f32>(converted_capacity)?;
    let mono_bytes = bytes::<f32>(frames)?;
    let converter_peak = if rate == 44_100 {
        admit(&[mono_bytes], available)?
    } else {
        admit(&[mono_bytes, converted_bytes, fft_bytes], available)?
    };
    let widening_bound = admit(&[converted_bytes, bytes::<f64>(output)?], available)?;
    let qm_working_bytes = flitzis_looper_analysis::default_bpm_working_bytes(output)
        .ok_or(PcmError::Limit("QM workspace extent"))?;
    let inference_bound = admit(&[bytes::<f64>(output)?, qm_working_bytes], available)?;
    let mut mono = reserve::<f32>(frames)?;
    let mut loaded_hash = Sha256::new();
    let mut normal_hash = Sha256::new();
    let mut nn_hash = Sha256::new();
    for chunk in sample.samples.chunks(CHUNK * sample.channels) {
        check_cancelled(cancelled)?;
        for frame in chunk.chunks_exact(sample.channels) {
            // Preserve the existing NN sum's +0 seed, including signed-zero bits.
            let mut nn_sum = 0.0_f64;
            for &value in frame {
                if !value.is_finite() {
                    return Err(PcmError::InvalidInput("QM nonfinite playback"));
                }
                loaded_hash.update(value.to_le_bytes());
                nn_sum += f64::from(value);
            }
            nn_hash.update(((nn_sum / sample.channels as f64) as f32).to_le_bytes());
            let value = normal_mean(frame);
            if !value.is_finite() {
                return Err(PcmError::InvalidInput("QM nonfinite normal mono"));
            }
            normal_hash.update(value.to_le_bytes());
            mono.push(value);
        }
    }
    if format!("{:x}", nn_hash.finalize()) != verified_nn_mono {
        return Err(PcmError::InvalidInput(
            "QM playback differs from verified NN mono",
        ));
    }
    check_cancelled(cancelled)?;
    let pcm_limit = available
        .checked_sub(FIXED)
        .and_then(|n| n.checked_sub(fft_bytes))
        .ok_or(PcmError::Limit("QM converter admission"))?;
    let converted = resample_mono_cancellable(mono, rate, 44_100, pcm_limit, cancelled)?;
    if converted.len() != output || converted.capacity() > converted_capacity {
        return Err(PcmError::Limit("QM converter capacity differs"));
    }
    let widening_peak = admit(
        &[bytes::<f32>(converted.capacity())?, bytes::<f64>(output)?],
        available,
    )?;
    let mut samples = reserve::<f64>(output)?;
    let mut analyzer_hash = Sha256::new();
    for chunk in converted.chunks(CHUNK) {
        check_cancelled(cancelled)?;
        for &value in chunk {
            let value = f64::from(value);
            if !value.is_finite() {
                return Err(PcmError::InvalidInput("QM nonfinite analyzer"));
            }
            analyzer_hash.update(value.to_le_bytes());
            samples.push(value);
        }
    }
    drop(converted);
    let inference_peak = admit(
        &[bytes::<f64>(samples.capacity())?, qm_working_bytes],
        available,
    )?;
    check_cancelled(cancelled)?;
    Ok(QmPcm {
        samples,
        loaded_interleaved_sha256: format!("{:x}", loaded_hash.finalize()),
        normal_mono_sha256: format!("{:x}", normal_hash.finalize()),
        analyzer_sha256: format!("{:x}", analyzer_hash.finalize()),
        resampler: if rate == 44_100 {
            "identity-44100-v1"
        } else {
            "rubato-fft-1.0-44100-delay-trim-tail-flush-v1"
        },
        qm_working_bytes,
        preparation_peak_bytes: converter_peak
            .max(widening_bound)
            .max(inference_bound)
            .max(widening_peak)
            .max(inference_peak),
    })
}

#[cfg(test)]
#[path = "qm_tests.rs"]
mod tests;
