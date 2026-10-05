//! Musical key detection using CNN-based inference (KeyNet / MusicalKeyCNN).
//!
//! Pipeline: mono audio → CQT spectrogram → ONNX inference → Camelot key string.
//!
//! # Usage
//!
//! ```ignore
//! use flitzis_looper_analysis::{detect_key, KeyError};
//!
//! let samples: Vec<f32> = /* mono audio at 44100 Hz */;
//! match detect_key(&samples, 44100) {
//!     Ok(result) => println!("Key: {} (confidence: {:.2}%)", result.key_name, result.confidence * 100.0),
//!     Err(KeyError::InsufficientData) => eprintln!("Audio too short"),
//!     Err(KeyError::SilentAudio) => eprintln!("Audio is silent"),
//!     Err(KeyError::ModelError(msg)) => eprintln!("Model error: {msg}"),
//!     Err(KeyError::CqtError(msg)) => eprintln!("CQT error: {msg}"),
//! }
//! ```

mod cqt;
mod inference;
pub mod librosa_cqt;
mod mapping;

pub use cqt::CqtParams;
pub use mapping::camelot_index_to_key;

/// Frequency rows accepted by the bundled KeyNet model after trimming the CQT.
const KEYNET_INPUT_BINS: usize = 104;

/// Result of key detection.
#[derive(Debug, Clone)]
pub struct KeyResult {
    /// Musical key string (e.g., "Am", "C", "A#m").
    pub key_name: String,
    /// Confidence of the prediction (softmax of top logit, 0.0–1.0).
    pub confidence: f32,
}

/// Errors from key detection.
#[derive(Debug, Clone)]
pub enum KeyError {
    /// Audio buffer is too short for CQT analysis.
    InsufficientData,
    /// Audio buffer contains only silence.
    SilentAudio,
    /// ONNX model failed to load or run.
    ModelError(String),
    /// CQT computation failed.
    CqtError(String),
}

/// Compute softmax of a single logit value relative to all logits.
///
/// Numerically stable: subtracts max logit before exponentiating.
fn softmax_single(logits: &[f32], index: usize) -> f32 {
    let max_logit = logits.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let exp_i = (logits[index] - max_logit).exp();
    let sum_exp: f32 = logits.iter().map(|l| (l - max_logit).exp()).sum();
    if sum_exp == 0.0 {
        1.0 / logits.len() as f32 // Uniform fallback
    } else {
        exp_i / sum_exp
    }
}

/// Detect the musical key of a mono audio buffer.
///
/// # Arguments
/// * `samples` - Mono audio samples as f32 (range typically -1.0 to 1.0).
/// * `sample_rate_hz` - Sample rate of the audio (must be 44100 Hz).
///
/// # Returns
/// A `KeyResult` with the detected key name and confidence,
/// or a `KeyError` if detection fails.
///
/// # Errors
/// * `InsufficientData` — audio is shorter than one CQT hop (8820 samples at 44100 Hz).
/// * `SilentAudio` — all samples are zero or near-zero.
/// * `ModelError` — ONNX model failed to load or run.
/// * `CqtError` — CQT computation failed.
pub fn detect_key(samples: &[f32], sample_rate_hz: u32) -> Result<KeyResult, KeyError> {
    // 1. Validate input length.
    // Minimum: at least one hop_length worth of samples to produce a CQT frame.
    let min_samples = CqtParams::default().hop_length;
    if samples.len() < min_samples {
        return Err(KeyError::InsufficientData);
    }

    // 2. Check for silence (all samples within epsilon of zero).
    let epsilon = 1e-6f32;
    let is_silent = samples.iter().all(|s| s.abs() < epsilon);
    if is_silent {
        return Err(KeyError::SilentAudio);
    }

    // 3. Compute CQT spectrogram.
    let params = CqtParams::default();
    let cqt_data = cqt::compute_cqt(samples, sample_rate_hz, &params);

    // CQT returns empty vec if signal is too short for any frames.
    if cqt_data.is_empty() {
        return Err(KeyError::InsufficientData);
    }

    // 4. Calculate number of time frames from CQT output shape.
    let n_time_frames = cqt_data.len() / KEYNET_INPUT_BINS;
    if n_time_frames == 0 {
        return Err(KeyError::InsufficientData);
    }

    // 5. Run ONNX inference.
    let (predicted_class, logits) = inference::run_inference(&cqt_data, n_time_frames)?;

    // 6. Map class index to key name.
    let key_name = mapping::camelot_index_to_key(predicted_class)
        .unwrap_or("unknown")
        .to_string();

    // 7. Compute confidence (softmax of predicted class).
    let confidence = softmax_single(&logits, predicted_class);

    Ok(KeyResult {
        key_name,
        confidence,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine_wave(freq_hz: f32, sample_rate: u32, duration_secs: f32) -> Vec<f32> {
        let n = (sample_rate as f32 * duration_secs) as usize;
        (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * freq_hz * i as f32 / sample_rate as f32).sin())
            .collect()
    }

    #[test]
    fn test_detect_key_insufficient_data() {
        let samples = vec![1.0f32; 100]; // Far too short
        let result = detect_key(&samples, 44_100);
        assert!(matches!(result, Err(KeyError::InsufficientData)));
    }

    #[test]
    fn test_detect_key_silent_audio() {
        let samples = vec![0.0f32; 100_000]; // Long enough but silent
        let result = detect_key(&samples, 44_100);
        assert!(matches!(result, Err(KeyError::SilentAudio)));
    }

    #[test]
    fn test_detect_key_near_silent_audio() {
        let samples = vec![1e-8f32; 100_000]; // Near-silent (below epsilon)
        let result = detect_key(&samples, 44_100);
        assert!(matches!(result, Err(KeyError::SilentAudio)));
    }

    #[test]
    fn test_detect_key_exactly_one_hop() {
        // Exactly one hop_length of non-silent data → 1 CQT frame.
        // With no model file, expect ModelError. With model, expect Ok.
        let hop = CqtParams::default().hop_length;
        let samples = sine_wave(440.0, 44_100, hop as f32 / 44_100.0);
        let result = detect_key(&samples, 44_100);
        match result {
            Ok(r) => {
                assert!(!r.key_name.is_empty());
                assert!(r.confidence > 0.0 && r.confidence <= 1.0);
            }
            Err(KeyError::InsufficientData) => {
                // Acceptable: CQT produced 0 frames
            }
            Err(KeyError::ModelError(msg)) => {
                // Acceptable: model not found in test environment
                assert!(
                    msg.contains("not found") || msg.contains("failed to load"),
                    "unexpected model error: {msg}"
                );
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn test_detect_key_sine_wave_no_model() {
        // 5 seconds of 440 Hz sine — enough data, but no model → ModelError
        let samples = sine_wave(440.0, 44_100, 5.0);
        let result = detect_key(&samples, 44_100);
        match result {
            Err(KeyError::ModelError(msg)) => {
                assert!(
                    msg.contains("not found") || msg.contains("failed to load"),
                    "expected model error, got: {msg}"
                );
            }
            Ok(r) => {
                // Model exists and detection succeeded — also valid
                assert!(!r.key_name.is_empty());
                assert!(r.confidence > 0.0 && r.confidence <= 1.0);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn test_softmax_single_uniform() {
        let logits = vec![0.0f32; 24];
        for i in 0..24 {
            let conf = softmax_single(&logits, i);
            assert!(
                (conf - 1.0 / 24.0).abs() < 1e-5,
                "uniform softmax should be ~{:.5}, got {conf}",
                1.0 / 24.0
            );
        }
    }

    #[test]
    fn test_softmax_single_dominant() {
        let mut logits = vec![-2.0f32; 24];
        logits[5] = 10.0; // Dominant class
        let conf = softmax_single(&logits, 5);
        assert!(
            conf > 0.99,
            "dominant class should have confidence > 0.99, got {conf}"
        );
        let other_conf = softmax_single(&logits, 0);
        assert!(
            other_conf < 0.001,
            "non-dominant class should have confidence < 0.001, got {other_conf}"
        );
    }

    #[test]
    fn test_softmax_single_sum_to_one() {
        let logits = vec![
            1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 1.5, 2.5, 3.5, 4.5, 5.5, 6.5, 7.5,
            8.5, 9.5, 10.5, 0.5, 1.2, 3.3, 5.5,
        ];
        let sum: f32 = (0..24).map(|i| softmax_single(&logits, i)).sum();
        assert!(
            (sum - 1.0).abs() < 1e-4,
            "softmax values should sum to 1.0, got {sum}"
        );
    }

    #[test]
    fn test_key_result_fields() {
        let result = KeyResult {
            key_name: "Am".into(),
            confidence: 0.85,
        };
        assert_eq!(result.key_name, "Am");
        assert!((result.confidence - 0.85).abs() < 1e-5);
    }

    #[test]
    fn test_key_error_clone() {
        let err = KeyError::ModelError("test".into());
        let cloned = err.clone();
        assert!(matches!(cloned, KeyError::ModelError(_)));
    }
}
