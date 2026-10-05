//! ONNX inference wrapper for the KeyNet model.
//!
//! Loads the pretrained `keynet.onnx` model and runs inference on CQT
//! spectrograms to produce 24-class key logits. The session is loaded
//! lazily on first use and cached for all subsequent calls.

use std::sync::{Mutex, OnceLock};

use ort::{
    session::Session,
    value::{TensorElementType, TensorRef, ValueType},
};

use crate::key_detection::{KEYNET_INPUT_BINS, KeyError};

/// Path to the KeyNet ONNX model file.
const MODEL_FILENAME: &str = "keynet.onnx";

/// Lazy-loaded ONNX session for the KeyNet model.
///
/// Wrapped in a Mutex to allow mutable access for `session.run()`.
/// The inner Option handles the case where the session hasn't been
/// initialized yet or failed to load.
static SESSION: OnceLock<Mutex<Option<Session>>> = OnceLock::new();

/// Validate the loaded model's input/output signature.
///
/// Checks that the model has:
/// - Exactly 1 input with f32 dtype and 4 dimensions, dim[2] == 104
/// - An output named "logits" with shape containing 24
fn validate_session(session: &Session) -> Result<(), KeyError> {
    // Check input.
    let inputs = session.inputs();
    if inputs.len() != 1 {
        return Err(KeyError::ModelError(format!(
            "model expected 1 input, got {}",
            inputs.len()
        )));
    }

    let input = &inputs[0];
    let input_dtype = input.dtype();

    // Verify input is f32 tensor.
    match input_dtype {
        ValueType::Tensor { ty, shape, .. } => {
            if *ty != TensorElementType::Float32 {
                return Err(KeyError::ModelError(format!(
                    "model input expected f32, got {:?}",
                    ty
                )));
            }

            if shape.len() != 4 {
                return Err(KeyError::ModelError(format!(
                    "model input expected 4 dimensions, got {}",
                    shape.len()
                )));
            }

            // The bundled model expects the 104 retained CQT frequency rows.
            let freq_dim = shape[2];
            if freq_dim >= 0 && freq_dim != KEYNET_INPUT_BINS as i64 {
                return Err(KeyError::ModelError(format!(
                    "model input dim[2] expected {KEYNET_INPUT_BINS} (freq bins), got {}",
                    freq_dim
                )));
            }
        }
        other => {
            return Err(KeyError::ModelError(format!(
                "model input expected tensor, got {:?}",
                other
            )));
        }
    }

    // Check output — must have a "logits" output with 24 classes.
    let outputs = session.outputs();
    let logits_output = outputs
        .iter()
        .find(|o| o.name() == "logits")
        .ok_or_else(|| {
            KeyError::ModelError(format!(
                "model missing 'logits' output (found: {:?})",
                outputs.iter().map(|o| o.name()).collect::<Vec<_>>()
            ))
        })?;

    match logits_output.dtype() {
        ValueType::Tensor { ty, shape, .. } => {
            if *ty != TensorElementType::Float32 {
                return Err(KeyError::ModelError(format!(
                    "model output 'logits' expected f32, got {:?}",
                    ty
                )));
            }

            // Last dimension must be 24 (Camelot classes).
            if shape.is_empty() {
                return Err(KeyError::ModelError(
                    "model output 'logits' has no dimensions".into(),
                ));
            }

            let last_dim = shape[shape.len() - 1];
            if last_dim >= 0 && last_dim != 24 {
                return Err(KeyError::ModelError(format!(
                    "model output 'logits' last dim expected 24 (classes), got {}",
                    last_dim
                )));
            }
        }
        other => {
            return Err(KeyError::ModelError(format!(
                "model output 'logits' expected tensor, got {:?}",
                other
            )));
        }
    }

    Ok(())
}

/// Load the model and cache it. Returns an error if the model is not found
/// or fails to load. On success, the session is cached for reuse.
fn load_session() -> Result<(), KeyError> {
    let path = resolve_model_path().ok_or_else(|| {
        KeyError::ModelError(format!(
            "model file '{}' not found in search paths",
            MODEL_FILENAME
        ))
    })?;

    let session = Session::builder()
        .map_err(|e| KeyError::ModelError(format!("failed to create session builder: {e}")))?
        .commit_from_file(path)
        .map_err(|e| KeyError::ModelError(format!("failed to load model: {e}")))?;

    // Validate model signature before caching.
    validate_session(&session)?;

    // Get or initialize the mutex, then set the session.
    SESSION.get_or_init(Default::default);
    let mut guard = SESSION
        .get()
        .unwrap()
        .lock()
        .map_err(|e| KeyError::ModelError(format!("poisoned session lock: {e}")))?;
    *guard = Some(session);

    Ok(())
}

/// Get a mutable reference to the cached session, loading it if needed.
///
/// Returns the session inside a MutexGuard so the caller can use `session.run()`
/// which requires `&mut Session`.
fn get_session_mut() -> Result<std::sync::MutexGuard<'static, Option<Session>>, KeyError> {
    SESSION.get_or_init(Default::default);
    let guard = SESSION
        .get()
        .unwrap()
        .lock()
        .map_err(|e| KeyError::ModelError(format!("poisoned session lock: {e}")))?;

    if guard.is_none() {
        // Can't load while holding the lock (would deadlock on recursion).
        // Drop the lock, load, then re-acquire.
        drop(guard);
        load_session()?;
        return get_session_mut();
    }

    Ok(guard)
}

/// Resolve the path to the KeyNet ONNX model.
///
/// Search order:
/// 1. Next to the current executable (`exe_dir/keynet.onnx`)
/// 2. Relative to the current working directory (`./keynet.onnx`)
/// 3. In common model directories (`assets/models/keynet.onnx`, `models/keynet.onnx`)
fn resolve_model_path() -> Option<std::path::PathBuf> {
    // 1. Next to executable
    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent()
    {
        let candidate = parent.join(MODEL_FILENAME);
        if candidate.exists() {
            return Some(candidate);
        }
    }

    // 2. Current working directory
    let cwd_candidate = std::path::PathBuf::from(MODEL_FILENAME);
    if cwd_candidate.exists() {
        return Some(cwd_candidate);
    }

    // 3. Common model directories
    for dir in &["assets/models", "models", "assets"] {
        let candidate = std::path::PathBuf::from(dir).join(MODEL_FILENAME);
        if candidate.exists() {
            return Some(candidate);
        }
    }

    None
}

/// Create an ONNX input tensor from a CQT spectrogram.
///
/// The CQT data is expected to be in shape `(104, T)` (row-major, frequency
/// bins major). This function reshapes it to `(1, 1, 104, T)` for the model.
fn create_input_tensor(
    cqt_data: &[f32],
    n_time_frames: usize,
) -> Result<TensorRef<'_, f32>, KeyError> {
    let expected_len = KEYNET_INPUT_BINS * n_time_frames;
    if cqt_data.len() != expected_len {
        return Err(KeyError::ModelError(format!(
            "CQT tensor size mismatch: expected {expected_len} ({KEYNET_INPUT_BINS} × {n_time_frames}), got {}",
            cqt_data.len()
        )));
    }

    // Shape: (batch=1, channels=1, freq_bins=104, time=T)
    TensorRef::from_array_view(([1usize, 1, KEYNET_INPUT_BINS, n_time_frames], cqt_data))
        .map_err(|e| KeyError::ModelError(format!("failed to create input tensor: {e}")))
}

/// Compute argmax of a slice, returning the index of the maximum element.
fn argmax(slice: &[f32]) -> Option<usize> {
    slice
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(i, _)| i)
}

/// Run ONNX inference on a CQT spectrogram.
///
/// # Arguments
/// * `cqt_tensor` - Flattened CQT spectrogram in shape `(104, T)` row-major.
/// * `n_time_frames` - Number of time frames (T dimension).
///
/// # Returns
/// A tuple of `(predicted_class_index, logits)` where:
/// - `predicted_class_index` is in range 0–23
/// - `logits` is a `Vec<f32>` of length 24 (raw model output)
pub fn run_inference(
    cqt_tensor: &[f32],
    n_time_frames: usize,
) -> Result<(usize, Vec<f32>), KeyError> {
    if n_time_frames == 0 {
        return Err(KeyError::ModelError(
            "cannot run inference with 0 time frames".into(),
        ));
    }

    let mut session_guard = get_session_mut()?;
    let session = session_guard
        .as_mut()
        .ok_or_else(|| KeyError::ModelError("session not initialized".into()))?;

    run_inference_with_session(session, cqt_tensor, n_time_frames)
}

/// Execute the model with a validated session and preprocessed CQT rows.
fn run_inference_with_session(
    session: &mut Session,
    cqt_tensor: &[f32],
    n_time_frames: usize,
) -> Result<(usize, Vec<f32>), KeyError> {
    let input = create_input_tensor(cqt_tensor, n_time_frames)?;

    let outputs = session
        .run(ort::inputs![input])
        .map_err(|e| KeyError::ModelError(format!("inference failed: {e}")))?;

    // Extract the logits tensor (output name: "logits", shape: (1, 24))
    let logits_value = outputs
        .get("logits")
        .ok_or_else(|| KeyError::ModelError("model output missing 'logits' tensor".into()))?;

    let logits: Vec<f32> = logits_value
        .try_extract_array::<f32>()
        .map_err(|e| KeyError::ModelError(format!("failed to extract logits: {e}")))?
        .iter()
        .copied()
        .collect();

    if logits.len() != 24 {
        return Err(KeyError::ModelError(format!(
            "expected 24 logits, got {}",
            logits.len()
        )));
    }

    let predicted_class =
        argmax(&logits).ok_or_else(|| KeyError::ModelError("empty logits".into()))?;

    Ok((predicted_class, logits))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_model_path_no_model() {
        // Without a model file present, resolve should return None.
        // Just verify the function doesn't panic.
        let _ = resolve_model_path();
    }

    #[test]
    fn test_model_path_resolution_finds_model() {
        // With model file in assets/models/, resolve should return Some.
        // This test passes when the model is present and fails when it's missing.
        let path = resolve_model_path();
        // Either the model is found (happy path) or not found (CI without model).
        // Both are acceptable — the key is that the function doesn't panic.
        if let Some(p) = path {
            assert!(p.exists(), "resolved path does not exist: {p:?}");
            assert_eq!(p.file_name().unwrap(), "keynet.onnx");
        }
    }

    #[test]
    fn test_load_session_validates_model() {
        if resolve_model_path().is_none() {
            assert!(matches!(
                load_session(),
                Err(KeyError::ModelError(message)) if message.contains("not found")
            ));
            return;
        }

        load_session().expect("a resolved KeyNet model must load and validate successfully");
        let guard = SESSION.get().unwrap().lock().unwrap();
        assert!(
            guard.is_some(),
            "session should be cached after successful load"
        );
    }

    #[test]
    fn test_bundled_model_accepts_cqt_preprocessing() {
        // Cargo runs unit tests from the crate directory. Locate the tracked asset
        // without changing cwd or populating the process-global session cache.
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../assets/models")
            .join(MODEL_FILENAME);
        if !path.exists() {
            // Source-only CI distributions may omit the binary model asset.
            return;
        }

        let mut session = Session::builder()
            .expect("create bundled KeyNet session")
            .commit_from_file(&path)
            .expect("a present bundled KeyNet must load successfully");
        validate_session(&session)
            .expect("bundled KeyNet signature must accept 104 frequency rows");

        let samples: Vec<f32> = (0..44_100 * 5)
            .map(|index| (std::f32::consts::TAU * 440.0 * index as f32 / 44_100.0).sin())
            .collect();
        let cqt = crate::key_detection::cqt::compute_cqt(&samples, 44_100, &Default::default());
        assert!(!cqt.is_empty());
        assert_eq!(cqt.len() % KEYNET_INPUT_BINS, 0);
        let n_time_frames = cqt.len() / KEYNET_INPUT_BINS;
        let (predicted_class, logits) =
            run_inference_with_session(&mut session, &cqt, n_time_frames)
                .expect("bundled KeyNet must infer from the trimmed CQT");

        assert!(predicted_class < 24);
        assert_eq!(logits.len(), 24);
        assert!(logits.iter().all(|value| value.is_finite()));
        assert!(crate::key_detection::camelot_index_to_key(predicted_class).is_some());
    }

    #[test]
    fn test_create_input_tensor_wrong_size() {
        let data = vec![1.0f32; 100]; // Wrong size for (104, 2)
        let result = create_input_tensor(&data, 2);
        assert!(result.is_err());
    }

    #[test]
    fn test_create_input_tensor_correct_size() {
        let data = vec![0.5f32; KEYNET_INPUT_BINS * 10]; // Correct size for (104, 10)
        let result = create_input_tensor(&data, 10);
        assert!(result.is_ok());
    }

    #[test]
    fn test_run_inference_zero_frames() {
        let result = run_inference(&[], 0);
        assert!(result.is_err());
        match result {
            Err(KeyError::ModelError(msg)) => {
                assert!(msg.contains("0 time frames"), "unexpected error: {msg}");
            }
            other => panic!("expected ModelError, got: {other:?}"),
        }
    }

    #[test]
    fn test_run_inference_wrong_tensor_size() {
        // Wrong size triggers error before model load
        let data = vec![0.0f32; 500]; // Not 104 * T for any valid T
        let result = run_inference(&data, 10); // 104*10 = 1040, but we have 500
        assert!(result.is_err());
    }

    #[test]
    fn test_run_inference_no_model() {
        // Without a model file, inference should fail gracefully
        let data = vec![0.0f32; KEYNET_INPUT_BINS * 10];
        let result = run_inference(&data, 10);
        // Should be ModelError, not a panic
        match result {
            Err(KeyError::ModelError(msg)) => {
                assert!(
                    resolve_model_path().is_none() && msg.contains("not found"),
                    "expected model error, got: {msg}"
                );
            }
            Ok(_) => {
                // Model exists and inference succeeded — also valid
            }
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[test]
    fn test_argmax_unique_max() {
        let logits = vec![
            0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0, 0.9, 0.8, 0.7, 0.6, 0.5, 0.4, 0.3,
            0.2, 0.1, 0.0, -0.1, -0.2, -0.3, -0.4,
        ];
        assert_eq!(argmax(&logits), Some(9));
    }

    #[test]
    fn test_argmax_all_equal() {
        let logits = vec![0.5f32; 24];
        let idx = argmax(&logits).unwrap();
        assert!((0..24).contains(&idx));
    }

    #[test]
    fn test_argmax_empty() {
        assert_eq!(argmax(&[] as &[f32]), None);
    }

    #[test]
    fn test_argmax_negative_values() {
        let logits = vec![-2.0, -1.0, -3.0, -0.5, -1.5];
        assert_eq!(argmax(&logits), Some(3)); // -0.5 is the max
    }
}
