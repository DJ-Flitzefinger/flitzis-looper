//! Strict control-side projection parsing. Only the original Native delivery is
//! retained; Python dictionaries never reconstruct source or map authority.

use super::loader_events::LoaderEventDelivery;
use super::source_map_admission::SourceMapRow;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyFloat, PyInt, PyString};

const KEYS: [&str; 4] = [
    "sync_requested",
    "intent_revision",
    "delivery",
    "exact_quarters",
];

fn required<'py>(row: &Bound<'py, PyDict>, key: &str) -> PyResult<Bound<'py, PyAny>> {
    row.get_item(key)?
        .ok_or_else(|| PyValueError::new_err(format!("source map projection missing {key}")))
}

fn parse_row(row: &Bound<'_, PyDict>) -> PyResult<SourceMapRow> {
    // Checking both count and every required key rejects additions and renamed
    // fields before cloning any delivery owners.
    if row.len() != KEYS.len() && row.len() != KEYS.len() + 1 {
        return Err(PyValueError::new_err(
            "source map projection requires sync_requested, intent_revision, delivery, exact_quarters and optional ordinary_pending",
        ));
    }
    for (key, _) in row.iter() {
        let key = key
            .cast::<PyString>()
            .map_err(|_| PyValueError::new_err("source map projection keys must be strings"))?;
        if !KEYS.contains(&key.to_str()?) && key.to_str()? != "ordinary_pending" {
            return Err(PyValueError::new_err(
                "source map projection has an unknown key",
            ));
        }
    }
    for key in KEYS {
        if !row.contains(key)? {
            return Err(PyValueError::new_err(
                "source map projection requires sync_requested, intent_revision, delivery, exact_quarters and optional ordinary_pending",
            ));
        }
    }
    let ordinary_pending = match row.get_item("ordinary_pending")? {
        None => false,
        Some(value) if value.is_instance_of::<PyBool>() => value.extract::<bool>()?,
        Some(_) => {
            return Err(PyValueError::new_err(
                "source map ordinary_pending must be a strict boolean",
            ));
        }
    };
    let sync_requested = required(row, "sync_requested")?;
    if !sync_requested.is_instance_of::<PyBool>() {
        return Err(PyValueError::new_err(
            "source map sync_requested must be a strict boolean",
        ));
    }
    let intent_revision = required(row, "intent_revision")?;
    if intent_revision.is_instance_of::<PyBool>() || !intent_revision.is_instance_of::<PyInt>() {
        return Err(PyValueError::new_err(
            "source map intent_revision must be a strict u64 integer",
        ));
    }
    let intent_revision = intent_revision.extract::<u64>().map_err(|_| {
        PyValueError::new_err("source map intent_revision is outside the u64 domain")
    })?;
    let quarters = required(row, "exact_quarters")?;
    let exact_quarters = if quarters.is_none() {
        None
    } else {
        if quarters.is_instance_of::<PyBool>()
            || !(quarters.is_instance_of::<PyInt>() || quarters.is_instance_of::<PyFloat>())
        {
            return Err(PyValueError::new_err(
                "source map exact_quarters must be numeric or None",
            ));
        }
        let quarters = quarters.extract::<f64>().map_err(|_| {
            PyValueError::new_err("source map exact_quarters is outside its numeric domain")
        })?;
        if !quarters.is_finite() || quarters <= 0.0 {
            return Err(PyValueError::new_err(
                "source map exact_quarters must be finite and positive",
            ));
        }
        Some(quarters)
    };
    let delivery = required(row, "delivery")?;
    if ordinary_pending && !delivery.is_none() {
        return Err(PyValueError::new_err(
            "source map ordinary_pending requires delivery=None",
        ));
    }
    let delivery = if delivery.is_none() {
        None
    } else {
        let original = delivery
            .extract::<PyRef<'_, LoaderEventDelivery>>()
            .map_err(|_| {
                PyValueError::new_err(
                    "source map delivery must be an original LoaderEventDelivery or None",
                )
            })?;
        Some((*original).clone())
    };
    Ok(SourceMapRow {
        sync_requested: sync_requested.extract()?,
        intent_revision,
        delivery,
        exact_quarters,
        ordinary_pending,
    })
}

pub(super) fn parse_rows(py: Python<'_>, rows: Vec<Py<PyDict>>) -> PyResult<Vec<SourceMapRow>> {
    rows.iter().map(|row| parse_row(row.bind(py))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row<'py>(py: Python<'py>) -> Bound<'py, PyDict> {
        let row = PyDict::new(py);
        row.set_item("sync_requested", false).unwrap();
        row.set_item("intent_revision", 0).unwrap();
        row.set_item("delivery", py.None()).unwrap();
        row.set_item("exact_quarters", py.None()).unwrap();
        row
    }

    fn parses(py: Python<'_>, row: Bound<'_, PyDict>) -> PyResult<Vec<SourceMapRow>> {
        parse_rows(py, vec![row.unbind()])
    }

    #[test]
    fn optional_ordinary_permission_is_strict_and_legacy_rows_never_grant_it() {
        Python::initialize();
        Python::attach(|py| {
            let value = row(py);
            assert!(!parses(py, value.clone()).unwrap()[0].ordinary_pending);
            for permission in [true, false] {
                value.set_item("ordinary_pending", permission).unwrap();
                assert_eq!(
                    parses(py, value.clone()).unwrap()[0].ordinary_pending,
                    permission
                );
            }
            for invalid in [
                py.None(),
                1_i32.into_pyobject(py).unwrap().into_any().unbind(),
                "true".into_pyobject(py).unwrap().into_any().unbind(),
            ] {
                value.set_item("ordinary_pending", invalid).unwrap();
                assert!(parses(py, value.clone()).is_err());
            }
            value.set_item("ordinary_pending", true).unwrap();
            value.del_item("delivery").unwrap();
            assert!(parses(py, value).is_err());
        });
    }

    #[test]
    fn complete_rows_preserve_strict_scalars_without_fabricating_delivery() {
        Python::initialize();
        Python::attach(|py| {
            let value = row(py);
            let parsed = parses(py, value.clone()).unwrap();
            assert!(!parsed[0].sync_requested);
            assert_eq!(parsed[0].intent_revision, 0);
            assert_eq!(parsed[0].exact_quarters, None);
            assert!(parsed[0].delivery.is_none());
            value.set_item("sync_requested", true).unwrap();
            value.set_item("intent_revision", u64::MAX).unwrap();
            value.set_item("exact_quarters", 32).unwrap();
            let parsed = parses(py, value).unwrap();
            assert!(parsed[0].sync_requested);
            assert_eq!(parsed[0].intent_revision, u64::MAX);
            assert_eq!(parsed[0].exact_quarters, Some(32.0));
            assert!(parsed[0].delivery.is_none()); // Admission remains a separate check.
        });
    }

    #[test]
    fn missing_extra_and_renamed_fields_refuse_without_dto_reconstruction() {
        Python::initialize();
        Python::attach(|py| {
            for key in KEYS {
                let value = row(py);
                value.del_item(key).unwrap();
                assert!(parses(py, value).is_err());
            }
            let value = row(py);
            value.set_item("map_revision", "forged").unwrap();
            assert!(parses(py, value).is_err());
            let value = row(py);
            value.del_item("delivery").unwrap();
            value.set_item("artifact", PyDict::new(py)).unwrap();
            assert!(parses(py, value).is_err());
            for value in [
                PyDict::new(py).into_any(),
                "forged".into_pyobject(py).unwrap().into_any(),
            ] {
                let projection = row(py);
                projection.set_item("delivery", value).unwrap();
                assert!(parses(py, projection).is_err());
            }
        });
    }

    #[test]
    fn boolean_integer_and_numeric_domains_are_not_coerced() {
        Python::initialize();
        Python::attach(|py| {
            for expression in [c"0", c"1", c"'true'", c"None"] {
                let value = row(py);
                value
                    .set_item("sync_requested", py.eval(expression, None, None).unwrap())
                    .unwrap();
                assert!(parses(py, value).is_err());
            }
            for expression in [c"True", c"-1", c"18446744073709551616", c"1.0", c"'1'"] {
                let value = row(py);
                value
                    .set_item("intent_revision", py.eval(expression, None, None).unwrap())
                    .unwrap();
                assert!(parses(py, value).is_err());
            }
            for expression in [
                c"True",
                c"0",
                c"-0.0",
                c"-1",
                c"float('inf')",
                c"float('nan')",
                c"'32'",
            ] {
                let value = row(py);
                value
                    .set_item("exact_quarters", py.eval(expression, None, None).unwrap())
                    .unwrap();
                assert!(parses(py, value).is_err());
            }
            let value = row(py);
            value.set_item("exact_quarters", 0.125).unwrap();
            assert_eq!(parses(py, value).unwrap()[0].exact_quarters, Some(0.125));
        });
    }

    #[test]
    fn custom_equality_keys_cannot_impersonate_projection_field_names() {
        Python::initialize();
        Python::attach(|py| {
            let value = row(py);
            value.del_item("sync_requested").unwrap();
            let alias = py.eval(c"type('Alias', (), {'__hash__': lambda self: hash('sync_requested'), '__eq__': lambda self, other: other == 'sync_requested'})()", None, None).unwrap();
            value.set_item(alias, false).unwrap();
            assert!(value.contains("sync_requested").unwrap());
            let error = parses(py, value).unwrap_err();
            assert!(error.to_string().contains("keys must be strings"));
        });
    }
}
