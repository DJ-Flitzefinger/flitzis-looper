//! Real canonical material restore with public Engine polling during config I/O.

use super::*;
use crate::audio_engine::{
    material_migration::prepare_material_complete_for_test, material_migration_control as control,
    material_migration_recovery::MigrationProjectGuard,
};

#[test]
fn public_native_background_borrow_material_restore_and_supersession() {
    for supersession in ["none", "request", "unload"] {
        let (directory, engine, mut callback, original, _envelope) = engine();
        accept_current_source_timing(&engine, &mut callback);
        let root = directory.path().join("samples");
        let old = root.join("material-input.wav");
        let protected = std::fs::read(&original).unwrap();
        std::fs::write(&old, &protected).unwrap();
        let material = prepare_material_complete_for_test(&root, &old, RATE, 1, &|| false).unwrap();
        let metadata = material.metadata();
        assert!(!metadata["material_creation"].is_null());
        let creation = metadata["material_creation"].clone();
        let reference = metadata["new_reference"].as_str().unwrap().to_owned();
        let preparation = Python::attach(|py| {
            control::prepared_for_test(&engine.borrow(py), material, &root).unwrap()
        });
        let source = Python::attach(|py| {
            let borrowed = engine.borrow(py);
            control::adopt_for_format(
                &borrowed,
                0,
                &preparation,
                borrowed.control_producer().unwrap().clone(),
                (1, RATE, root.clone()),
            )
            .unwrap()
        });
        wait_until(|| callback.consumer.peek().is_ok());
        assert_eq!(source.phase().unwrap(), "pending");
        assert_eq!(callback.drain(), 1);
        let mut success = false;
        wait_until(|| {
            Python::attach(|py| {
                let borrowed = engine.borrow(py);
                while let Ok(event) = borrowed
                    .loader_rx
                    .lock()
                    .unwrap()
                    .try_recv()
                    .map(LoaderEvent::into_test_payload)
                {
                    match event {
                        LoaderEvent::Success { request_id, .. }
                            if request_id == source.request_id() =>
                        {
                            success = true
                        }
                        LoaderEvent::Error { error, .. } => {
                            panic!("genuine material adoption failed: {error}")
                        }
                        _ => {}
                    }
                }
                success && borrowed.cold_loading[0].load(Ordering::Acquire) == 0
            })
        });
        assert!(source.is_current().unwrap());
        preparation.release_preparation().unwrap();
        accept_current_source_timing(&engine, &mut callback);
        let actual_original = Python::attach(|py| {
            engine.borrow(py).cold_leases.lock().unwrap()[0]
                .as_ref()
                .unwrap()
                .original_path
                .clone()
        });
        assert_eq!(std::fs::read(&actual_original).unwrap(), protected);
        let config = root.join("flitzis_looper.config.json");
        let instance = "1".repeat(32);
        let mut paths = vec![serde_json::Value::Null; NUM_SAMPLES];
        let mut content = vec![serde_json::Value::Null; NUM_SAMPLES];
        paths[0] = serde_json::json!(reference);
        content[0] = serde_json::json!({"instance_id":instance, "material_id":metadata["material_id"], "material_creation":creation});
        let config_bytes =
            serde_json::to_vec(&serde_json::json!({"sample_paths":paths,"pad_content":content}))
                .unwrap();
        std::fs::write(&config, &config_bytes).unwrap();
        let (mut project, inventory) = Python::attach(|py| {
            let project = MigrationProjectGuard::new(
                py,
                root.to_string_lossy().into(),
                config.to_string_lossy().into(),
            )
            .unwrap();
            let inventory = project.lock_inventory(py).unwrap();
            (project, Py::new(py, inventory).unwrap())
        });
        let args = Python::attach(|py| {
            (
                0,
                creation.to_string(),
                config.to_string_lossy().into_owned(),
                instance,
                inventory.clone_ref(py),
                Option::<String>::None,
            )
                .into_pyobject(py)
                .unwrap()
                .unbind()
        });
        let original_stop = OriginalStopFeedback::capture(&engine, 0);
        let result = held_call(
            &engine,
            &mut callback,
            "restore_material_creation",
            args,
            WorkPoint::MaterialRestoreIo,
            |engine, callback| {
                Python::attach(|py| match supersession {
                    "request" => {
                        let binding = engine
                            .call_method1(py, "current_input_runtime_pad_binding", (0,))
                            .unwrap();
                        assert!(!binding.is_none(py));
                        engine
                            .call_method1(
                                py,
                                "capture_current_constant_timing",
                                (binding, 0.05, "independent fixture matching tolerance"),
                            )
                            .unwrap();
                    }
                    "unload" => {
                        engine.call_method1(py, "unload_sample", (0,)).unwrap();
                    }
                    "none" => {}
                    _ => unreachable!(),
                });
                if supersession == "unload" {
                    assert_eq!(callback.drain(), 1);
                }
                callback.render();
            },
        );
        match supersession {
            "none" => {
                assert!(Python::attach(|py| result.unwrap().is_none(py)));
                assert!(source.is_current().unwrap());
                Python::attach(|py| {
                    let borrowed = engine.borrow(py);
                    let leases = borrowed.cold_leases.lock().unwrap();
                    assert_eq!(
                        serde_json::to_value(
                            leases[0]
                                .as_ref()
                                .unwrap()
                                .material_creation_receipt()
                                .unwrap()
                        )
                        .unwrap(),
                        creation
                    );
                });
            }
            "request" => assert!(
                result
                    .unwrap_err()
                    .contains("source or request changed during verification")
            ),
            "unload" => {
                assert!(result.unwrap_err().contains("fresh current source missing"));
                Python::attach(|py| {
                    let message = engine.call_method0(py, "receive_msg").unwrap();
                    original_stop.assert_message(
                        &engine.borrow(py),
                        &message.extract::<PyRef<AudioMessage>>(py).unwrap(),
                        None,
                    );
                });
            }
            _ => unreachable!(),
        }
        assert!(callback.consumer.peek().is_err());
        callback.render();
        assert_eq!(std::fs::read(&config).unwrap(), config_bytes);
        assert_eq!(std::fs::read(&old).unwrap(), protected);
        assert_eq!(std::fs::read(&original).unwrap(), protected);
        assert_eq!(std::fs::read(&actual_original).unwrap(), protected);
        Python::attach(|py| inventory.borrow_mut(py).release());
        project.release();
    }
}
