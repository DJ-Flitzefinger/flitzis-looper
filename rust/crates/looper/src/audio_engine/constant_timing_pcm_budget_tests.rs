//! Bounded admission and captured ownership, without device startup or giant PCM.

use super::tests::{source, test_engine};
use super::*;
use crate::audio_engine::input_runtime_binding;

#[test]
fn capture_python_budget_rejects_bool_and_malformed_values_before_stream_or_request() {
    let engine = test_engine();
    let binding = input_runtime_binding::capture(&engine, 0).unwrap().unwrap();
    let request = engine.pad_request_ids.lock().unwrap()[0];
    let epoch = engine.prepared_source_epochs[0].load(Ordering::Acquire);
    Python::attach(|py| {
        let binding = Py::new(py, binding).unwrap();
        let engine = Py::new(py, engine).unwrap();
        let kwargs = PyDict::new(py);
        for expression in [c"True", c"False", c"None", c"1.5", c"'1073741824'"] {
            kwargs
                .set_item("pcm_limit_bytes", py.eval(expression, None, None).unwrap())
                .unwrap();
            let error = engine
                .bind(py)
                .call_method(
                    "capture_current_constant_timing",
                    (&binding, 0.001, "independent fixture bound"),
                    Some(&kwargs),
                )
                .unwrap_err();
            assert!(error.is_instance_of::<pyo3::exceptions::PyTypeError>(py));
        }
        for expression in [c"0", c"-1", c"1073741825", c"18446744073709551616"] {
            kwargs
                .set_item("pcm_limit_bytes", py.eval(expression, None, None).unwrap())
                .unwrap();
            let error = engine
                .bind(py)
                .call_method(
                    "capture_current_constant_timing",
                    (&binding, 0.001, "independent fixture bound"),
                    Some(&kwargs),
                )
                .unwrap_err();
            assert!(error.is_instance_of::<PyValueError>(py));
        }
        let owner = engine.borrow(py);
        assert_eq!(owner.pad_request_ids.lock().unwrap()[0], request);
        assert_eq!(
            owner.prepared_source_epochs[0].load(Ordering::Acquire),
            epoch
        );
    });
}

fn timing_bound() -> TimingBound {
    TimingBound {
        halfwidth_seconds: 0.001,
        provenance: "independent hardware-free fixture bound".into(),
    }
}

#[test]
fn pcm_budget_rejection_preserves_request_epoch_and_current_source() {
    let engine = test_engine();
    let binding = input_runtime_binding::capture(&engine, 0).unwrap().unwrap();
    let request = engine.pad_request_ids.lock().unwrap()[0];
    let epoch = engine.prepared_source_epochs[0].load(Ordering::Acquire);
    for limit in [0, 1, 1024 * 1024 * 1024 + 1, usize::MAX] {
        assert!(
            capture_preparation_with_limit(&engine, 0, timing_bound(), Some(&binding), limit)
                .is_err()
        );
        assert_eq!(engine.pad_request_ids.lock().unwrap()[0], request);
        assert_eq!(
            engine.prepared_source_epochs[0].load(Ordering::Acquire),
            epoch
        );
        assert!(binding.current());
    }
}

#[test]
fn explicit_captured_budget_rejects_retired_owner_and_newer_request() {
    for retired_authority in [false, true] {
        let engine = test_engine();
        let binding = input_runtime_binding::capture(&engine, 0).unwrap().unwrap();
        let request = engine.pad_request_ids.lock().unwrap()[0];
        if retired_authority {
            engine
                .input_runtime_ownership
                .revoke(0, engine.input_runtime_ownership.next_authority(0).unwrap());
        } else {
            engine.input_runtime_ownership.revoke_source(0);
        }
        assert!(
            capture_preparation_with_limit(
                &engine,
                0,
                timing_bound(),
                Some(&binding),
                1024 * 1024 * 1024,
            )
            .is_err()
        );
        assert_eq!(engine.pad_request_ids.lock().unwrap()[0], request);
    }
    let engine = test_engine();
    let binding = input_runtime_binding::capture(&engine, 0).unwrap().unwrap();
    let captured = capture_preparation_with_limit(
        &engine,
        0,
        timing_bound(),
        Some(&binding),
        1024 * 1024 * 1024,
    )
    .unwrap();
    let next_binding = input_runtime_binding::capture(&engine, 0).unwrap().unwrap();
    capture_preparation(&engine, 0, timing_bound(), Some(&next_binding)).unwrap();
    assert!(prepare_captured(&engine, &captured).is_err());
}

#[test]
fn actual_captured_preparation_retains_its_explicit_budget_without_publication() {
    let engine = test_engine();
    let binding = input_runtime_binding::capture(&engine, 0).unwrap().unwrap();
    let captured = capture_preparation_with_limit(
        &engine,
        0,
        timing_bound(),
        Some(&binding),
        32 * 1024 * 1024,
    )
    .unwrap();
    assert_eq!(captured.pcm_budget.limit_bytes(), 32 * 1024 * 1024);
    let ticket = prepare_captured(&engine, &captured).unwrap();
    assert_eq!(ticket.pcm_budget, captured.pcm_budget);
    assert_eq!(ticket.request_id, captured.request_id);
    assert!(Arc::ptr_eq(
        &ticket.sample.samples,
        &captured.sample.samples
    ));
    assert_eq!(ticket.sample.samples.len(), source().samples.len());
    assert_eq!(ticket.publication_status().unwrap(), "captured");
    assert!(engine.current_constant_timing.lock().unwrap()[0].is_empty());
    Python::attach(|py| {
        let metadata = ticket.metadata(py).unwrap();
        assert_eq!(
            metadata
                .bind(py)
                .get_item("pcm_limit_bytes")
                .unwrap()
                .extract::<usize>()
                .unwrap(),
            32 * 1024 * 1024
        );
    });
}
