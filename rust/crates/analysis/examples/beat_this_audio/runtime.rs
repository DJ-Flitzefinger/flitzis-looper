//! Fresh standalone process bootstrap only. Never configure the application's ORT.
use serde::Deserialize;
#[cfg(any(feature = "native-inference-dynamic", test))]
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    path::{Path, PathBuf},
};
#[cfg(any(feature = "native-inference-dynamic", test))]
use std::{fs::OpenOptions, io::Read, path::Component};

#[cfg(any(feature = "native-inference-dynamic", test))]
const MAX_MANIFEST: u64 = 4096;
#[cfg(any(feature = "native-inference-dynamic", test))]
const MAX_RUNTIME: u64 = 512 * 1024 * 1024;
pub const MANIFEST_ENV: &str = "FLITZIS_TRIAL_ORT_MANIFEST";
pub const DIGEST_ENV: &str = "FLITZIS_TRIAL_ORT_MANIFEST_SHA256";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Spec {
    schema_version: u32,
    os: String,
    arch: String,
    library: String,
    bytes: u64,
    sha256: String,
    version: String,
}

#[cfg(any(feature = "native-inference-dynamic", test))]
fn digest_valid(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

#[cfg(any(feature = "native-inference-dynamic", test))]
impl Spec {
    fn parse(raw: &[u8]) -> Result<Self, String> {
        let spec: Self = serde_json::from_slice(raw).map_err(|e| e.to_string())?;
        if spec.schema_version != 1
            || spec.os != std::env::consts::OS
            || spec.arch != std::env::consts::ARCH
        {
            return Err("unsupported runtime manifest schema or process OS/architecture".into());
        }
        let parts: Vec<_> = spec.version.split('.').collect();
        if parts.len() != 3
            || parts[0] != "1"
            || parts
                .iter()
                .any(|p| p.is_empty() || !p.bytes().all(|c| c.is_ascii_digit()))
            || parts[1]
                .parse::<u32>()
                .ok()
                .is_none_or(|n| n < ort::MINOR_VERSION)
        {
            return Err("unsupported exact runtime version".into());
        }
        let path = Path::new(&spec.library);
        if spec.library.contains(['/', '\\', ':', '\0'])
            || !matches!(path.components().next(), Some(Component::Normal(_)))
            || path.components().count() != 1
        {
            return Err("runtime library must be one package-local file name".into());
        }
        if spec.bytes == 0 || spec.bytes > MAX_RUNTIME || !digest_valid(&spec.sha256) {
            return Err("invalid runtime size or digest".into());
        }
        Ok(spec)
    }
}

struct CheckedFile {
    path: PathBuf,
    _file: File,
}

#[cfg(any(feature = "native-inference-dynamic", test))]
impl CheckedFile {
    fn open(path: &Path, expected: &str, limit: u64, size: Option<u64>) -> Result<Self, String> {
        if !path.is_absolute() || !digest_valid(expected) {
            return Err("runtime admission requires an absolute path and lowercase SHA256".into());
        }
        let path = path.canonicalize().map_err(|e| e.to_string())?;
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            // Read sharing only: deny writes/renames/deletes of this opened file.
            options.share_mode(1);
        }
        let mut file = options.open(&path).map_err(|e| e.to_string())?;
        let metadata = file.metadata().map_err(|e| e.to_string())?;
        if !metadata.is_file()
            || metadata.len() == 0
            || metadata.len() > limit
            || size.is_some_and(|n| n != metadata.len())
        {
            return Err("runtime file size/type mismatch".into());
        }
        let mut hash = Sha256::new();
        let mut bytes = 0u64;
        let mut buffer = [0u8; 65536];
        loop {
            let count = file.read(&mut buffer).map_err(|e| e.to_string())?;
            if count == 0 {
                break;
            }
            bytes += count as u64;
            if bytes > limit || bytes > metadata.len() {
                return Err("runtime file changed during verification".into());
            }
            hash.update(&buffer[..count]);
        }
        if bytes != metadata.len() || format!("{:x}", hash.finalize()) != expected {
            return Err("runtime file digest/extent mismatch".into());
        }
        Ok(Self { path, _file: file })
    }
}

/// Retained files pin the exact checked bytes through all CLI sessions/work.
/// On other OSes the caller must keep the owned directory immutable. Even on
/// Windows these file checks do not attest OS-mapped images or dependent DLLs.
pub struct Admission {
    manifest: CheckedFile,
    runtime: CheckedFile,
    spec: Spec,
    manifest_sha256: String,
}

impl Admission {
    #[cfg(any(feature = "native-inference-dynamic", test))]
    fn verify(path: &Path, digest: &str) -> Result<Self, String> {
        use std::io::{Seek, SeekFrom};
        let mut manifest = CheckedFile::open(path, digest, MAX_MANIFEST, None)?;
        manifest
            ._file
            .seek(SeekFrom::Start(0))
            .map_err(|e| e.to_string())?;
        let mut raw = Vec::new();
        (&mut manifest._file)
            .take(MAX_MANIFEST + 1)
            .read_to_end(&mut raw)
            .map_err(|e| e.to_string())?;
        // Recheck the exact bytes parsed, including on systems without deny-write.
        if raw.len() as u64 > MAX_MANIFEST || format!("{:x}", Sha256::digest(&raw)) != digest {
            return Err("runtime manifest changed before parsing".into());
        }
        let spec = Spec::parse(&raw)?;
        let directory = manifest
            .path
            .parent()
            .ok_or("runtime package has no directory")?;
        let runtime = CheckedFile::open(
            &directory.join(&spec.library),
            &spec.sha256,
            MAX_RUNTIME,
            Some(spec.bytes),
        )?;
        if runtime.path.parent() != Some(directory) {
            return Err("runtime library escapes its owned package directory".into());
        }
        Ok(Self {
            manifest,
            runtime,
            spec,
            manifest_sha256: digest.into(),
        })
    }

    pub fn receipt(&self) -> serde_json::Value {
        serde_json::json!({"schema_version":self.spec.schema_version,"library":self.spec.library,
            "manifest_sha256":self.manifest_sha256,
            "runtime_sha256":self.spec.sha256,"runtime_bytes":self.spec.bytes,
            "runtime_version":self.spec.version,"runtime_path":self.runtime.path,
            "manifest_path":self.manifest.path,"os":self.spec.os,"arch":self.spec.arch,
            "ort_api":ort::MINOR_VERSION,"evidence":"checked-files-not-mapped-image-or-full-producer"})
    }
}

/// Exactly one early call in a fresh CLI, before any other ORT API. The ort
/// wrapper caches its first library globally; this is not an App/reload API.
pub fn configure(
    request: Option<(&Path, &str)>,
) -> Result<(Option<Admission>, ort::environment::EnvironmentBuilder), String> {
    #[cfg(feature = "native-inference-dynamic")]
    {
        use libloading::Library;
        use std::{
            ffi::CStr,
            sync::atomic::{AtomicBool, Ordering},
        };
        static ATTEMPTED: AtomicBool = AtomicBool::new(false);
        let (path, digest) =
            request.ok_or("dynamic trial requires an explicit runtime manifest and SHA256")?;
        let admission = Admission::verify(path, digest)?;
        if ATTEMPTED.swap(true, Ordering::SeqCst) {
            return Err("runtime bootstrap requires a fresh standalone process".into());
        }
        eprintln!("native_ort_runtime_phase verified_files");
        // SAFETY: explicit caller-authorized native bytes were verified above;
        // immutable package custody is required. This local handle remains alive
        // until init_from has retained its process-global reference to the DLL.
        let library =
            unsafe { Library::new(&admission.runtime.path) }.map_err(|e| e.to_string())?;
        let base = unsafe {
            let get = library
                .get::<unsafe extern "C" fn() -> *const ort::sys::OrtApiBase>(b"OrtGetApiBase\0")
                .map_err(|e| e.to_string())?;
            get().as_ref().ok_or("null runtime API base")?
        };
        let version = unsafe {
            let version = (base.GetVersionString)();
            if version.is_null() {
                return Err("null runtime version".into());
            }
            CStr::from_ptr(version)
                .to_str()
                .map_err(|e| e.to_string())?
        };
        if version != admission.spec.version
            || unsafe { (base.GetApi)(ort::MINOR_VERSION) }.is_null()
        {
            return Err("runtime exact version/API mismatch".into());
        }
        eprintln!("native_ort_runtime_phase verified_version_api");
        let environment = ort::init_from(&admission.runtime.path).map_err(|e| e.to_string())?;
        Ok((Some(admission), environment))
    }
    #[cfg(not(feature = "native-inference-dynamic"))]
    {
        if request.is_some() {
            return Err("static CPU trial does not accept dynamic runtime configuration".into());
        }
        Ok((None, ort::init()))
    }
}

pub fn commit(environment: ort::environment::EnvironmentBuilder) -> Result<(), String> {
    if !environment.commit() {
        return Err("runtime environment already configured; request did not take effect".into());
    }
    eprintln!("native_ort_runtime_phase configured_environment");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn spec() -> serde_json::Value {
        serde_json::json!({"schema_version":1,"os":std::env::consts::OS,"arch":std::env::consts::ARCH,
            "library":"runtime.dll","bytes":7,"sha256":"a".repeat(64),"version":"1.24.4"})
    }
    #[test]
    fn manifest_rejects_foreign_target_and_schema() {
        for field in ["os", "arch", "schema_version"] {
            let mut value = spec();
            value[field] = serde_json::json!("foreign");
            assert!(Spec::parse(&serde_json::to_vec(&value).unwrap()).is_err());
        }
    }
    #[test]
    fn manifest_rejects_escape_size_digest_and_api() {
        for (field, value) in [
            ("library", serde_json::json!("../runtime.dll")),
            ("library", serde_json::json!("C:runtime.dll")),
            ("library", serde_json::json!("..\\runtime.dll")),
            ("bytes", serde_json::json!(0)),
            ("bytes", serde_json::json!(MAX_RUNTIME + 1)),
            ("sha256", serde_json::json!("A".repeat(64))),
            ("version", serde_json::json!("1.23.9")),
            ("version", serde_json::json!("1.24.4\n")),
        ] {
            let mut input = spec();
            input[field] = value;
            assert!(
                Spec::parse(&serde_json::to_vec(&input).unwrap()).is_err(),
                "{field}"
            );
        }
        assert!(Spec::parse(&serde_json::to_vec(&spec()).unwrap()).is_ok());
    }
    #[test]
    fn manifest_rejects_duplicate_unknown_and_partial_fields() {
        let raw = serde_json::to_string(&spec()).unwrap();
        assert!(Spec::parse(format!("{{\"bytes\":7,{}", &raw[1..]).as_bytes()).is_err());
        let mut input = spec();
        input["extra"] = serde_json::json!(true);
        assert!(Spec::parse(&serde_json::to_vec(&input).unwrap()).is_err());
        assert!(Spec::parse(b"{}").is_err());
    }
    #[test]
    fn runtime_configuration_cannot_be_silently_repeated() {
        commit(ort::init()).unwrap();
        assert!(
            commit(ort::init())
                .unwrap_err()
                .contains("did not take effect")
        );
    }
    #[test]
    fn relative_runtime_manifest_fails_before_open() {
        assert!(
            Admission::verify(Path::new("relative.json"), &"a".repeat(64))
                .err()
                .unwrap()
                .contains("absolute")
        );
    }
    #[test]
    fn checked_file_enforces_actual_extent_digest_and_windows_pin() {
        let directory = std::env::temp_dir().join(format!(
            "ort-file-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("runtime.bin");
        std::fs::write(&path, b"runtime bytes").unwrap();
        let digest = format!("{:x}", Sha256::digest(b"runtime bytes"));
        assert!(CheckedFile::open(&path, &digest, 12, None).is_err());
        assert!(CheckedFile::open(&path, &digest, 13, Some(12)).is_err());
        assert!(CheckedFile::open(&path, &"0".repeat(64), 13, Some(13)).is_err());
        let pin = CheckedFile::open(&path, &digest, 13, Some(13)).unwrap();
        #[cfg(windows)]
        {
            assert!(OpenOptions::new().write(true).open(&path).is_err());
            assert!(std::fs::remove_file(&path).is_err());
        }
        drop(pin);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
}
