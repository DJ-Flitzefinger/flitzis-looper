//! Opt-in dependency evidence before the original service test starts rendering.

use sha2::{Digest, Sha256};
use std::ffi::c_void;
use std::path::PathBuf;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn K32EnumProcessModules(
        process: *mut c_void,
        modules: *mut *mut c_void,
        bytes: u32,
        needed: *mut u32,
    ) -> i32;
    fn GetModuleFileNameW(module: *mut c_void, path: *mut u16, capacity: u32) -> u32;
}

pub(super) fn assert_selected() {
    let Some(request) = std::env::var_os("FLITZIS_SERVICE_DEPENDENCIES") else {
        return;
    };
    let request: serde_json::Value = serde_json::from_str(&request.into_string().unwrap()).unwrap();
    let directory = PathBuf::from(request["directory"].as_str().unwrap())
        .canonicalize()
        .unwrap();
    let expected = request["hashes"].as_object().unwrap();
    let names = [
        "rubberband-3.dll",
        "samplerate.dll",
        "sleef.dll",
        "sleefdft.dll",
    ];
    assert_eq!(expected.len(), names.len());
    for name in names {
        let hash = expected[name].as_str().unwrap();
        assert!(hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()));
    }
    let mut modules = [std::ptr::null_mut(); 256];
    let mut needed = 0;
    let bytes = std::mem::size_of_val(&modules) as u32;
    // Only query this isolated test process; no library loading or path repair.
    assert_ne!(
        unsafe {
            K32EnumProcessModules(
                -1_isize as *mut c_void,
                modules.as_mut_ptr(),
                bytes,
                &mut needed,
            )
        },
        0
    );
    assert!(
        needed <= bytes && (needed as usize).is_multiple_of(std::mem::size_of::<*mut c_void>())
    );
    let mut found = std::collections::BTreeMap::new();
    for module in modules
        .into_iter()
        .take(needed as usize / std::mem::size_of::<*mut c_void>())
    {
        let mut buffer = vec![0_u16; 32768];
        let length =
            unsafe { GetModuleFileNameW(module, buffer.as_mut_ptr(), buffer.len() as u32) };
        assert!(length > 0 && (length as usize) < buffer.len());
        let path = PathBuf::from(String::from_utf16(&buffer[..length as usize]).unwrap());
        let name = path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_ascii_lowercase();
        if !names.contains(&name.as_str()) {
            continue;
        }
        let actual = path.canonicalize().unwrap();
        assert_eq!(actual, directory.join(&name).canonicalize().unwrap());
        let hash = format!("{:x}", Sha256::digest(std::fs::read(&actual).unwrap()));
        assert_eq!(hash, expected[&name].as_str().unwrap().to_ascii_lowercase());
        assert!(
            found
                .insert(name, serde_json::json!({"path":actual,"file_sha256":hash}))
                .is_none(),
            "ambiguous duplicate dependency modules"
        );
    }
    assert_eq!(found.len(), names.len());
    eprintln!(
        "native_service_dependencies {}",
        serde_json::json!({
            "loaded_modules_and_current_file_hashes":found,
            "in_memory_image_attestation":false,
            "before_lifecycle_and_render":true,
        })
    );
}
