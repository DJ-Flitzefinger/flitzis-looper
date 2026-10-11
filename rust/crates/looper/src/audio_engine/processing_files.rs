//! Off-thread producer provenance. These witnesses grant no asset/adoption/cleanup authority.

use super::{cold_store, project_assets};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

// Both real producer manifests include owned cancellation/process helpers and
// verified tool images. Keep this off-thread inventory finite without omitting
// executed inputs from the complete lineage (currently at least 37 files).
const MAX_PROCESSING_FILES: usize = 64;

struct ProcessingFile {
    path: PathBuf,
    reader: File,
    identity: project_assets::FileIdentity,
    sha256: String,
    bytes: u64,
    _directories: Vec<File>,
}

/// Complete model/config witnesses held against writes and replacement during execution.
#[pyclass]
pub struct SealedProcessingFiles {
    files: Vec<ProcessingFile>,
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

impl SealedProcessingFiles {
    fn capture(paths: Vec<String>) -> io::Result<Self> {
        if paths.is_empty() || paths.len() > MAX_PROCESSING_FILES {
            return Err(invalid(
                "processing provenance requires one to sixty-four exact files",
            ));
        }
        let mut files = Vec::with_capacity(paths.len());
        for path in paths {
            if path
                .split(['/', '\\'])
                .any(|part| [".", ".."].contains(&part))
            {
                return Err(invalid("processing file contains lexical traversal"));
            }
            let input = Path::new(&path);
            let input = if input.is_absolute() {
                input.to_owned()
            } else {
                std::env::current_dir()?.join(input)
            };
            let directories = project_assets::directory_guards(
                input
                    .parent()
                    .ok_or_else(|| invalid("processing parent missing"))?,
            )?;
            let mut reader = cold_store::sealed_reader(&input)?;
            let before = reader.metadata()?;
            if !before.is_file() {
                return Err(invalid("processing witness requires a regular file"));
            }
            let identity = project_assets::file_identity(&reader)?;
            let (sha256, bytes) = cold_store::source_hash(&mut reader, &|| false)?;
            let canonical = std::fs::canonicalize(&input)?;
            if bytes != before.len()
                || reader.metadata()?.len() != bytes
                || project_assets::file_identity(&reader)? != identity
                || project_assets::capture_identity(&canonical)? != Some(identity.clone())
            {
                return Err(invalid(
                    "processing file changed during complete verification",
                ));
            }
            if files
                .iter()
                .any(|file: &ProcessingFile| file.identity == identity)
            {
                return Err(invalid(
                    "processing provenance repeats the same file identity",
                ));
            }
            files.push(ProcessingFile {
                path: canonical,
                reader,
                identity,
                sha256,
                bytes,
                _directories: directories,
            });
        }
        Ok(Self { files })
    }

    fn verify_files(&mut self) -> io::Result<()> {
        for file in &mut self.files {
            project_assets::reject_links(&file.path)?;
            if project_assets::capture_identity(&file.path)? != Some(file.identity.clone()) {
                return Err(invalid(
                    "processing file pathname no longer names its held identity",
                ));
            }
            cold_store::verify_file(&mut file.reader, &file.sha256, file.bytes, &|| false)?;
            if project_assets::file_identity(&file.reader)? != file.identity {
                return Err(invalid("processing file held identity changed"));
            }
        }
        Ok(())
    }
}

#[pymethods]
impl SealedProcessingFiles {
    #[new]
    fn new(py: Python<'_>, paths: Vec<String>) -> PyResult<Self> {
        py.detach(|| Self::capture(paths))
            .map_err(|error| PyValueError::new_err(error.to_string()))
    }

    /// Actual complete file witnesses; held readers remain live on this object.
    pub fn witnesses(&self) -> Vec<(String, String, u64)> {
        self.files
            .iter()
            .map(|file| {
                (
                    file.path.to_string_lossy().into_owned(),
                    file.sha256.clone(),
                    file.bytes,
                )
            })
            .collect()
    }

    /// Recheck every held complete file and its pathname before successful publication.
    pub fn verify(&mut self, py: Python<'_>) -> PyResult<()> {
        py.detach(|| self.verify_files())
            .map_err(|error| PyValueError::new_err(error.to_string()))
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn processing_witness_holds_actual_model_and_config_until_execution_returns() {
        let directory = tempfile::tempdir().unwrap();
        let model = directory.path().join("weights.bin");
        let config = directory.path().join("config.json");
        std::fs::write(&model, [7_u8; 65537]).unwrap();
        std::fs::write(&config, b"{\"quality\":1}").unwrap();
        let mut witness = SealedProcessingFiles::capture(vec![
            model.to_string_lossy().into_owned(),
            config.to_string_lossy().into_owned(),
        ])
        .unwrap();
        assert_eq!(witness.witnesses()[0].2, 65537);
        assert!(
            std::fs::OpenOptions::new()
                .write(true)
                .open(&model)
                .is_err()
        );
        assert!(std::fs::rename(&model, directory.path().join("replacement.bin")).is_err());
        assert!(std::fs::remove_file(&config).is_err());
        witness.verify_files().unwrap();
        drop(witness);
        std::fs::write(&model, [9_u8; 65537]).unwrap();
        let next =
            SealedProcessingFiles::capture(vec![model.to_string_lossy().into_owned()]).unwrap();
        assert_ne!(next.witnesses()[0].1, {
            use sha2::{Digest, Sha256};
            format!("{:x}", Sha256::digest([7_u8; 65537]))
        });
    }

    #[test]
    fn processing_witness_rejects_duplicate_identity_and_invalid_inventory() {
        let directory = tempfile::tempdir().unwrap();
        let model = directory.path().join("weights.bin");
        std::fs::write(&model, b"model").unwrap();
        let model = model.to_string_lossy().into_owned();
        assert!(SealedProcessingFiles::capture(vec![]).is_err());
        assert!(
            SealedProcessingFiles::capture(vec![model.clone(); MAX_PROCESSING_FILES + 1])
                .err()
                .unwrap()
                .to_string()
                .contains("sixty-four")
        );
        assert!(SealedProcessingFiles::capture(vec![model.clone(); 2]).is_err());
        assert!(SealedProcessingFiles::capture(vec![format!("{model}/../weights.bin")]).is_err());
        assert!(
            SealedProcessingFiles::capture(vec![directory.path().to_string_lossy().into_owned()])
                .is_err()
        );
        let paths = (0..MAX_PROCESSING_FILES)
            .map(|index| {
                let path = directory.path().join(format!("producer-{index}.py"));
                std::fs::write(&path, format!("producer = {index}\n")).unwrap();
                path.to_string_lossy().into_owned()
            })
            .collect();
        assert_eq!(
            SealedProcessingFiles::capture(paths).unwrap().files.len(),
            MAX_PROCESSING_FILES
        );
    }
}
