//! Optional declared ORT component custody, not native loader/closure attestation.
use super::{CheckedFile, digest_valid};
use serde::Deserialize;
use std::{collections::HashSet, fs::File, path::Path};

const MAX_MANIFEST: u64 = 16 * 1024;
const MAX_COMPONENT: u64 = 2 * 1024 * 1024 * 1024;
const MAX_TOTAL: u64 = 8 * 1024 * 1024 * 1024;
const MAX_FILES: usize = 64;

#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Role {
    OrtRuntime,
    OrtProviderShared,
    OrtProviderCuda,
    Cuda,
    Cudnn,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Component {
    name: String,
    role: Role,
    bytes: u64,
    sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Spec {
    schema_version: u32,
    os: String,
    arch: String,
    components: Vec<Component>,
}

impl Spec {
    fn parse(raw: &[u8], runtime: &super::Spec) -> Result<Self, String> {
        let spec: Self = serde_json::from_slice(raw).map_err(|e| e.to_string())?;
        if spec.schema_version != 1 || spec.os != runtime.os || spec.arch != runtime.arch {
            return Err("unsupported ORT package schema or process OS/architecture".into());
        }
        if spec.components.is_empty() || spec.components.len() > MAX_FILES {
            return Err("invalid ORT package component count".into());
        }
        let mut names = HashSet::new();
        let mut total = 0u64;
        let mut matched_runtime = 0;
        for item in &spec.components {
            // Portable ASCII basenames also reject Windows ADS, trailing-dot/space
            // aliases and separators on a Unix host. Case aliases fail on every OS.
            if item.name.is_empty()
                || item.name.len() > 128
                || item.name == "."
                || item.name == ".."
                || item.name.ends_with('.')
                || !item
                    .name
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
                || !names.insert(item.name.to_ascii_lowercase())
                || item.bytes == 0
                || item.bytes > MAX_COMPONENT
                || !digest_valid(&item.sha256)
            {
                return Err("invalid or duplicate ORT package component".into());
            }
            total = total
                .checked_add(item.bytes)
                .ok_or("ORT package size overflow")?;
            if total > MAX_TOTAL {
                return Err("ORT package exceeds declared byte bound".into());
            }
            if item.role == Role::OrtRuntime {
                matched_runtime += 1;
                if item.name != runtime.library
                    || item.bytes != runtime.bytes
                    || item.sha256 != runtime.sha256
                {
                    return Err("ORT package runtime differs from admitted runtime".into());
                }
            }
        }
        if matched_runtime != 1 {
            return Err("ORT package requires exactly one matching runtime component".into());
        }
        Ok(spec)
    }
}

// Identity comes from the SAME opened handle, not a second pathname query.
#[cfg(windows)]
fn identity(file: &File) -> Result<(u64, [u8; 16]), String> {
    use std::os::windows::io::AsRawHandle;
    #[repr(C)]
    struct Information {
        volume: u64,
        id: [u8; 16],
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetFileInformationByHandleEx(
            handle: *mut std::ffi::c_void,
            class: i32,
            information: *mut std::ffi::c_void,
            bytes: u32,
        ) -> i32;
    }
    let mut info = std::mem::MaybeUninit::<Information>::uninit();
    // SAFETY: a live File handle and writable correctly laid out Win32 structure.
    // FileIdInfo=18 returns the full128-bit ID, including on ReFS. No weaker
    // legacy64-bit fallback when the filesystem cannot provide this identity.
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            18,
            info.as_mut_ptr().cast(),
            std::mem::size_of::<Information>() as u32,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let info = unsafe { info.assume_init() };
    Ok((info.volume, info.id))
}

#[cfg(unix)]
fn identity(file: &File) -> Result<(u64, u64), String> {
    use std::os::unix::fs::MetadataExt;
    let info = file.metadata().map_err(|e| e.to_string())?;
    Ok((info.dev(), info.ino()))
}

#[cfg(not(any(windows, unix)))]
fn identity(_: &File) -> Result<(u64, u64), String> {
    Err("ORT package file identity is unsupported on this OS".into())
}

pub(super) struct Admission {
    manifest: CheckedFile,
    files: Vec<CheckedFile>,
    spec: Spec,
    sha256: String,
}

impl Admission {
    pub(super) fn verify(
        path: &Path,
        digest: &str,
        runtime: &super::Admission,
    ) -> Result<Self, String> {
        let mut manifest = CheckedFile::open(path, digest, MAX_MANIFEST, None)?;
        let directory = runtime
            .manifest
            .path
            .parent()
            .ok_or("ORT package has no directory")?;
        if manifest.path.parent() != Some(directory) {
            return Err("ORT package manifest must share the runtime package directory".into());
        }
        let raw = manifest.read_json(digest, MAX_MANIFEST)?;
        let spec = Spec::parse(&raw, &runtime.spec)?;
        let mut identities = HashSet::from([
            identity(&manifest._file)?,
            identity(&runtime.manifest._file)?,
        ]);
        let runtime_identity = identity(&runtime.runtime._file)?;
        let mut files = Vec::with_capacity(spec.components.len());
        for item in &spec.components {
            let file = CheckedFile::open(
                &directory.join(&item.name),
                &item.sha256,
                MAX_COMPONENT,
                Some(item.bytes),
            )?;
            if file.path.parent() != Some(directory) {
                return Err("ORT component escapes its owned package directory".into());
            }
            let id = identity(&file._file)?;
            if !identities.insert(id) || (item.role == Role::OrtRuntime) != (id == runtime_identity)
            {
                return Err("duplicate or mismatched ORT package file identity".into());
            }
            files.push(file);
        }
        Ok(Self {
            manifest,
            files,
            spec,
            sha256: digest.into(),
        })
    }

    pub(super) fn receipt(&self) -> serde_json::Value {
        let components: Vec<_> = self.spec.components.iter().zip(&self.files).map(|(item, file)| {
            let role = match item.role {
                Role::OrtRuntime => "ort-runtime",
                Role::OrtProviderShared => "ort-provider-shared",
                Role::OrtProviderCuda => "ort-provider-cuda",
                Role::Cuda => "cuda",
                Role::Cudnn => "cudnn",
            };
            serde_json::json!({"name":item.name,"role":role,"bytes":item.bytes,"sha256":item.sha256,"path":file.path})
        }).collect();
        serde_json::json!({"schema_version":1,"manifest_path":self.manifest.path,
            "manifest_sha256":self.sha256,"os":self.spec.os,"arch":self.spec.arch,
            "declared_file_count":self.files.len(),"declared_file_bytes":self.spec.components.iter().map(|c| c.bytes).sum::<u64>(),
            "components":components,"evidence":"declared-checked-files-only",
            "loader_closure":"UNKNOWN","mapped_images":"UNKNOWN","full_producer":"UNKNOWN"})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn runtime() -> super::super::Spec {
        super::super::Spec::parse(
            serde_json::to_string(&serde_json::json!({"schema_version":1,
            "os":std::env::consts::OS,"arch":std::env::consts::ARCH,"library":"runtime.dll",
            "bytes":7,"sha256":"a".repeat(64),"version":"1.24.4"}))
            .unwrap()
            .as_bytes(),
        )
        .unwrap()
    }
    fn spec() -> serde_json::Value {
        serde_json::json!({"schema_version":1,"os":std::env::consts::OS,"arch":std::env::consts::ARCH,
            "components":[{"name":"runtime.dll","role":"ort-runtime","bytes":7,"sha256":"a".repeat(64)},
            {"name":"shared.dll","role":"ort-provider-shared","bytes":11,"sha256":"b".repeat(64)}]})
    }
    fn valid(value: &serde_json::Value) -> bool {
        Spec::parse(&serde_json::to_vec(value).unwrap(), &runtime()).is_ok()
    }
    #[test]
    fn package_manifest_rejects_aliases_roles_and_runtime_mismatch() {
        assert!(valid(&spec()));
        for (field, value) in [
            ("name", serde_json::json!("RUNTIME.DLL")),
            ("name", serde_json::json!("../shared.dll")),
            ("name", serde_json::json!("shared.dll.")),
            ("name", serde_json::json!("shared.dll:x")),
            ("role", serde_json::json!("unknown")),
            ("bytes", serde_json::json!(0)),
            ("bytes", serde_json::json!(MAX_COMPONENT + 1)),
            ("sha256", serde_json::json!("A".repeat(64))),
        ] {
            let mut input = spec();
            input["components"][1][field] = value;
            assert!(!valid(&input), "{field}");
        }
        for field in ["name", "bytes", "sha256"] {
            let mut input = spec();
            input["components"][0][field] = serde_json::json!("wrong");
            assert!(!valid(&input));
        }
        let mut input = spec();
        input["components"][1]["role"] = serde_json::json!("ort-runtime");
        assert!(!valid(&input));
        input["components"] = serde_json::json!([]);
        assert!(!valid(&input));
    }
    #[test]
    fn package_manifest_strict_fields_target_count_and_total() {
        for field in ["os", "arch", "schema_version"] {
            let mut input = spec();
            input[field] = serde_json::json!("foreign");
            assert!(!valid(&input));
        }
        let raw = serde_json::to_string(&spec()).unwrap();
        assert!(
            Spec::parse(
                format!("{{\"os\":\"foreign\",{}", &raw[1..]).as_bytes(),
                &runtime()
            )
            .is_err()
        );
        let mut input = spec();
        input["unknown"] = true.into();
        assert!(!valid(&input));
        input = spec();
        input["components"][1]["unknown"] = true.into();
        assert!(!valid(&input));
        assert!(Spec::parse(b"{}", &runtime()).is_err());
        for count in [5, MAX_FILES + 1] {
            let mut input = spec();
            let items = input["components"].as_array_mut().unwrap();
            for n in 0..count {
                items.push(serde_json::json!({"name":format!("cuda{n}.dll"),"role":"cuda","bytes":MAX_COMPONENT,"sha256":"c".repeat(64)}));
            }
            assert!(!valid(&input));
        }
    }

    #[test]
    fn package_custody_pins_all_files_and_rejects_aliases_and_changed_bytes() {
        use sha2::{Digest, Sha256};
        let directory = std::env::temp_dir().join(format!(
            "ort-package-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        let write = |name: &str, raw: &[u8]| {
            let path = directory.join(name);
            std::fs::write(&path, raw).unwrap();
            (path, format!("{:x}", Sha256::digest(raw)))
        };
        let (_, runtime_sha) = write("runtime.dll", b"runtime");
        let (shared, shared_sha) = write("shared.dll", b"shared file");
        let mut r = runtime();
        r.sha256 = runtime_sha;
        let raw = serde_json::to_vec(
            &serde_json::json!({"schema_version":1,"os":r.os,"arch":r.arch,
            "library":r.library,"bytes":r.bytes,"sha256":r.sha256,"version":r.version}),
        )
        .unwrap();
        let (runtime_manifest, digest) = write("runtime.json", &raw);
        let admitted = super::super::Admission::verify(&runtime_manifest, &digest).unwrap();
        let mut p = spec();
        p["components"][0]["sha256"] = r.sha256.into();
        p["components"][1]["sha256"] = shared_sha.into();
        let check = |value: &serde_json::Value| {
            let (path, digest) = write("package.json", &serde_json::to_vec(value).unwrap());
            Admission::verify(&path, &digest, &admitted)
        };
        let package = check(&p).unwrap();
        assert_eq!(package.receipt()["declared_file_bytes"], 18);
        #[cfg(windows)]
        for path in [&shared, &package.manifest.path, &runtime_manifest] {
            assert!(std::fs::OpenOptions::new().write(true).open(path).is_err());
            assert!(std::fs::remove_file(path).is_err());
        }
        drop(package);
        std::fs::hard_link(&shared, directory.join("alias.dll")).unwrap();
        let mut alias = p.clone();
        let mut item = alias["components"][1].clone();
        item["name"] = "alias.dll".into();
        alias["components"].as_array_mut().unwrap().push(item);
        assert!(check(&alias).err().unwrap().contains("identity"));
        let mut wrong = p.clone();
        wrong["components"][1]["bytes"] = 12.into();
        assert!(check(&wrong).is_err());
        wrong = p.clone();
        wrong["components"][1]["sha256"] = "0".repeat(64).into();
        assert!(check(&wrong).is_err());
        wrong = p.clone();
        wrong["components"][1]["name"] = "missing.dll".into();
        assert!(check(&wrong).is_err());
        let (large, digest) = write("oversize.json", &vec![b' '; MAX_MANIFEST as usize + 1]);
        assert!(Admission::verify(&large, &digest, &admitted).is_err());
        // Canonical directory containment is independent of matching file bytes.
        let nested = directory.join("nested");
        std::fs::create_dir(&nested).unwrap();
        let path = nested.join("package.json");
        std::fs::write(&path, serde_json::to_vec(&p).unwrap()).unwrap();
        let hash = format!("{:x}", Sha256::digest(std::fs::read(&path).unwrap()));
        assert!(Admission::verify(&path, &hash, &admitted).is_err());
        std::fs::write(&shared, b"changed file").unwrap();
        assert!(check(&p).is_err());
        drop(admitted);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
