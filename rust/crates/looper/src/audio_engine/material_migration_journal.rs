//! Immutable bounded journal records; reopening never restores native authority.
use super::{cold_store::sealed_reader, material_paths, project_assets};
use pyo3::{
    exceptions::{PyRuntimeError, PyValueError},
    prelude::*,
};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};

const JOURNAL_LIMIT: u64 = 16 * 1024 * 1024;
const JOURNAL_RECORDS: usize = 16;
const JOURNAL_TRANSACTIONS: usize = 256;

fn value_error(error: impl std::fmt::Display) -> PyErr {
    PyValueError::new_err(error.to_string())
}

#[pyclass]
pub struct MaterialMigrationJournalStore {
    path: PathBuf,
    identity: project_assets::FileIdentity,
    _guards: Vec<File>,
    sequence: Mutex<usize>,
}

impl MaterialMigrationJournalStore {
    fn at(samples: &Path, transaction: &str, create: bool) -> PyResult<Self> {
        if !material_paths::valid_id(transaction) {
            return Err(value_error("invalid migration transaction ID"));
        }
        let path = material_paths::resolve(
            samples,
            &samples.join(".material-migrations").join(transaction),
        )
        .map_err(value_error)?
        .path;
        let parent = path
            .parent()
            .ok_or_else(|| value_error("journal parent missing"))?;
        let mut guards = project_assets::directory_guards(samples).map_err(value_error)?;
        if create {
            match fs::create_dir(parent) {
                Ok(()) => (),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
                Err(error) => return Err(value_error(error)),
            }
        }
        guards.extend(project_assets::directory_guards(parent).map_err(value_error)?);
        if create {
            fs::create_dir(&path).map_err(value_error)?;
        }
        guards.extend(project_assets::directory_guards(&path).map_err(value_error)?);
        let identity = project_assets::capture_identity(&path)
            .map_err(value_error)?
            .ok_or_else(|| value_error("journal directory identity missing"))?;
        Ok(Self {
            path,
            identity,
            _guards: guards,
            sequence: Mutex::new(0),
        })
    }

    fn read_latest(&self) -> PyResult<(usize, Option<String>)> {
        let records = self.read_history()?;
        Ok((records.len(), records.last().cloned()))
    }

    fn read_history(&self) -> PyResult<Vec<String>> {
        let mut records = Vec::new();
        for entry in fs::read_dir(&self.path).map_err(value_error)? {
            let entry = entry.map_err(value_error)?;
            if records.len() >= JOURNAL_RECORDS {
                return Err(value_error("journal record capacity exceeded"));
            }
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| value_error("unknown journal child"))?;
            if name.len() != 7
                || !name.as_bytes()[..2].iter().all(u8::is_ascii_digit)
                || &name[2..] != ".json"
            {
                return Err(value_error("unknown journal child retained"));
            }
            let ordinal: usize = name[..2].parse().map_err(value_error)?;
            records.push((ordinal, entry.path()));
        }
        records.sort_by_key(|record| record.0);
        if records
            .iter()
            .enumerate()
            .any(|(expected, (actual, _))| expected != *actual)
        {
            return Err(value_error("journal has an incomplete phase sequence"));
        }
        let mut history = Vec::with_capacity(records.len());
        // Each recognized record is sealed and bounded; malformed partial writes remain visible.
        for (_, path) in &records {
            let reader = sealed_reader(path).map_err(value_error)?;
            if reader.metadata().map_err(value_error)?.len() > JOURNAL_LIMIT {
                return Err(value_error("journal record exceeds bounded size"));
            }
            let mut text = String::new();
            reader
                .take(JOURNAL_LIMIT + 1)
                .read_to_string(&mut text)
                .map_err(value_error)?;
            let _: serde_json::Value = serde_json::from_str(&text).map_err(value_error)?;
            history.push(text);
        }
        self.check_identity()?;
        Ok(history)
    }

    fn check_identity(&self) -> PyResult<()> {
        if project_assets::capture_identity(&self.path)
            .map_err(value_error)?
            .as_ref()
            != Some(&self.identity)
        {
            return Err(value_error("journal directory was replaced"));
        }
        Ok(())
    }
}

#[pymethods]
impl MaterialMigrationJournalStore {
    #[new]
    pub fn new(samples_root: String, transaction_id: String) -> PyResult<Self> {
        Self::at(Path::new(&samples_root), &transaction_id, true)
    }
    #[staticmethod]
    pub fn open(samples_root: String, transaction_id: String) -> PyResult<Self> {
        let store = Self::at(Path::new(&samples_root), &transaction_id, false)?;
        *store
            .sequence
            .lock()
            .map_err(|_| PyRuntimeError::new_err("journal sequence lock poisoned"))? =
            store.read_latest()?.0;
        Ok(store)
    }
    #[staticmethod]
    pub fn scan(samples_root: String) -> PyResult<Vec<String>> {
        let samples = Path::new(&samples_root);
        let _guards = project_assets::directory_guards(samples).map_err(value_error)?;
        let parent = samples.join(".material-migrations");
        if !parent.exists() {
            return Ok(Vec::new());
        }
        let _parent = project_assets::directory_guards(&parent).map_err(value_error)?;
        let mut output = Vec::new();
        for entry in fs::read_dir(&parent).map_err(value_error)? {
            let entry = entry.map_err(value_error)?;
            if output.len() >= JOURNAL_TRANSACTIONS {
                return Err(value_error("migration journal inventory capacity exceeded"));
            }
            let id = entry.file_name().to_string_lossy().into_owned();
            let result = Self::at(samples, &id, false).and_then(|store| store.read_latest());
            let value = match result {
                Ok((_, record)) => {
                    serde_json::json!({"transaction_id": id, "record": record, "error": null})
                }
                Err(error) => {
                    serde_json::json!({"transaction_id": id, "record": null, "error": error.to_string()})
                }
            };
            output.push(value.to_string());
        }
        Ok(output)
    }
    pub fn append(&self, content: String) -> PyResult<String> {
        if content.is_empty() || content.len() as u64 > JOURNAL_LIMIT {
            return Err(value_error("journal record exceeds bounded size"));
        }
        let _: serde_json::Value = serde_json::from_str(&content).map_err(value_error)?;
        let mut sequence = self
            .sequence
            .lock()
            .map_err(|_| PyRuntimeError::new_err("journal sequence lock poisoned"))?;
        if *sequence >= JOURNAL_RECORDS {
            return Err(value_error("journal phase capacity exhausted"));
        }
        self.check_identity()?;
        if self.read_history()?.len() != *sequence {
            return Err(value_error(
                "journal sequence changed before append; preserved",
            ));
        }
        let path = self.path.join(format!("{:02}.json", *sequence));
        let mut writer = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(value_error)?;
        writer
            .write_all(content.as_bytes())
            .and_then(|_| writer.sync_all())
            .map_err(value_error)?;
        drop(writer);
        let mut reader = sealed_reader(&path).map_err(value_error)?;
        let mut checked = String::new();
        reader.read_to_string(&mut checked).map_err(value_error)?;
        if checked != content {
            return Err(value_error("journal reopen verification failed"));
        }
        *sequence += 1;
        Ok(path.to_string_lossy().into_owned())
    }

    pub fn history(&self) -> PyResult<Vec<String>> {
        self.read_history()
    }

    /// Remove exact acknowledged metadata only after its caller transferred all
    /// recognized intent/evidence to a durable successor or completed cleanup.
    /// Unknown, partial, gapped and concurrently edited records are never removed.
    pub fn compact(&mut self, expected_records: Vec<String>) -> PyResult<()> {
        let sequence = self
            .sequence
            .lock()
            .map_err(|_| PyRuntimeError::new_err("journal sequence lock poisoned"))?;
        let current = self.read_history()?;
        if current.is_empty() || current != expected_records || current.len() != *sequence {
            return Err(value_error(
                "journal compaction evidence changed; preserved",
            ));
        }
        let mut leaves = Vec::with_capacity(current.len());
        let mut readers = Vec::with_capacity(current.len());
        for (index, text) in current.iter().enumerate() {
            let path = self.path.join(format!("{index:02}.json"));
            let mut reader = sealed_reader(&path).map_err(value_error)?;
            let mut bytes = Vec::new();
            reader.read_to_end(&mut bytes).map_err(value_error)?;
            if bytes != text.as_bytes() {
                return Err(value_error("journal changed before compaction; preserved"));
            }
            let proof = project_assets::VerifiedFile {
                name: format!("{index:02}.json"),
                identity: project_assets::file_identity(&reader).map_err(value_error)?,
                bytes: bytes.len() as u64,
                sha256: format!("{:x}", Sha256::digest(&bytes)),
            };
            readers.push(reader);
            leaves.push((path, proof));
        }
        self.check_identity()?;
        drop(readers);
        for (path, proof) in leaves {
            project_assets::remove_verified_file(&path, &proof).map_err(value_error)?;
        }
        self._guards.clear();
        project_assets::remove_empty_directory(&self.path, &self.identity).map_err(value_error)?;
        Ok(())
    }
}
