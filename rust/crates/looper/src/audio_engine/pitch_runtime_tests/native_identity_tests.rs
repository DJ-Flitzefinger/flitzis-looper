//! Read-only binding of this test process to its actual imported native runtime.

use crate::audio_engine::c3_process_metrics;
use crate::audio_engine::rubberband_backend::RubberBandLiveStateOpaque;
use serde_json::{Value, json};
use std::ffi::{c_char, c_float, c_void};
use std::path::PathBuf;

const GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT: u32 = 0x00000002;
const GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS: u32 = 0x00000004;

unsafe extern "C" {
    fn rubberband_live_shift(
        state: *mut RubberBandLiveStateOpaque,
        input: *const *const c_float,
        output: *const *mut c_float,
    );
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleExW(flags: u32, name_or_address: *const u16, module: *mut *mut c_void)
    -> i32;
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
    fn GetModuleFileNameW(module: *mut c_void, filename: *mut u16, size: u32) -> u32;
    fn GetProcAddress(module: *mut c_void, name: *const c_char) -> *mut c_void;
    fn ReadProcessMemory(
        process: *mut c_void,
        address: *const c_void,
        buffer: *mut c_void,
        size: usize,
        read: *mut usize,
    ) -> i32;
}

fn module_from_address(address: *const c_void) -> Result<*mut c_void, String> {
    let mut module = std::ptr::null_mut();
    let flags =
        GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT;
    // SAFETY: FROM_ADDRESS treats the second argument as an address, not a
    // string; module is a writable output. The ordinary native import keeps its
    // DLL loaded. No reference count changes, so never FreeLibrary this handle.
    let found = unsafe { GetModuleHandleExW(flags, address.cast(), &mut module) };
    if found == 0 || module.is_null() {
        return Err(format!(
            "native identity: rubberband_live_shift address module query failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(module)
}

fn read_process_bytes<const N: usize>(address: usize) -> Result<[u8; N], String> {
    address
        .checked_add(N)
        .ok_or_else(|| "native identity: process read address overflow".to_owned())?;
    let mut bytes = [0_u8; N];
    let mut read = 0;
    // SAFETY: -1 is the current-process pseudo-handle. Windows validates the
    // source range, and bytes exposes exactly N writable bytes. No arbitrary
    // pointer is dereferenced in Rust; this API only reads our own test process.
    let success = unsafe {
        ReadProcessMemory(
            -1_isize as *mut c_void,
            address as *const c_void,
            bytes.as_mut_ptr().cast(),
            N,
            &mut read,
        )
    };
    if success == 0 || read != N {
        return Err(format!(
            "native identity: bounded current-process read failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(bytes)
}

fn little_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

#[derive(Clone, Copy)]
struct ImportThunk {
    instruction: [u8; 6],
    slot: usize,
    slot_rva: usize,
    target: usize,
}

/// Resolve only the x64 FF25 RIP-relative import jump, with its slot proved to
/// belong to this loaded executable's declared PE32+ import address table.
fn resolve_import_thunk(address: usize, module: *mut c_void) -> Result<ImportThunk, String> {
    if !cfg!(target_arch = "x86_64") {
        return Err("native identity: executable import thunks are supported only on x64".into());
    }
    let instruction = read_process_bytes::<6>(address)?;
    if instruction[..2] != [0xff, 0x25] {
        return Err(
            "native identity: executable function is not a supported FF25 import thunk (static or unsupported binding)"
                .into(),
        );
    }

    // Microsoft PE32+ layout: e_lfanew at DOS+0x3c; PE signature followed by the
    // 20-byte COFF header; IAT data-directory address/size at Optional+208/212.
    // Address arithmetic is checked before reads; ReadProcessMemory validates
    // every fixed read. Full header extents are checked once SizeOfImage is read.
    // The 1 MiB/4 KiB header caps below are conservative probe acceptance limits.
    let base = module as usize;
    let dos = read_process_bytes::<64>(base)?;
    let pe_offset = little_u32(&dos, 0x3c) as usize;
    if dos[..2] != *b"MZ" || !(64..=1_048_576).contains(&pe_offset) {
        return Err("native identity: unsupported executable DOS/PE header offset".into());
    }
    let pe_address = base
        .checked_add(pe_offset)
        .ok_or_else(|| "native identity: PE header address overflow".to_owned())?;
    let pe = read_process_bytes::<24>(pe_address)?;
    let optional_size = usize::from(u16::from_le_bytes([pe[20], pe[21]]));
    if pe[..4] != *b"PE\0\0"
        || u16::from_le_bytes([pe[4], pe[5]]) != 0x8664
        || !(216..=4096).contains(&optional_size)
    {
        return Err("native identity: unsupported executable PE/x64 optional header".into());
    }
    let optional_address = pe_address
        .checked_add(24)
        .ok_or_else(|| "native identity: optional header address overflow".to_owned())?;
    let optional = read_process_bytes::<216>(optional_address)?;
    if u16::from_le_bytes([optional[0], optional[1]]) != 0x20b || little_u32(&optional, 108) < 13 {
        return Err("native identity: executable has no supported PE32+ IAT directory".into());
    }
    let image_size = little_u32(&optional, 56) as usize;
    let header_size = little_u32(&optional, 60) as usize;
    let header_end = pe_offset
        .checked_add(24)
        .and_then(|offset| offset.checked_add(optional_size))
        .ok_or_else(|| "native identity: PE header extent overflow".to_owned())?;
    let image_end = base
        .checked_add(image_size)
        .ok_or_else(|| "native identity: PE image extent overflow".to_owned())?;
    let instruction_end = address
        .checked_add(instruction.len())
        .ok_or_else(|| "native identity: import instruction extent overflow".to_owned())?;
    if header_end > header_size
        || header_size > image_size
        || address < base
        || instruction_end > image_end
    {
        return Err("native identity: PE header or import instruction exceeds its image".into());
    }
    let iat_rva = little_u32(&optional, 208) as usize;
    let iat_size = little_u32(&optional, 212) as usize;
    let iat_end_rva = iat_rva
        .checked_add(iat_size)
        .ok_or_else(|| "native identity: PE IAT extent overflow".to_owned())?;
    if iat_rva == 0 || iat_size < 8 || !iat_size.is_multiple_of(8) || iat_end_rva > image_size {
        return Err("native identity: executable IAT range is absent or invalid".into());
    }
    let displacement = i32::from_le_bytes([
        instruction[2],
        instruction[3],
        instruction[4],
        instruction[5],
    ]);
    let slot = instruction_end
        .checked_add_signed(displacement as isize)
        .ok_or_else(|| "native identity: RIP-relative import slot address overflow".to_owned())?;
    let slot_rva = slot
        .checked_sub(base)
        .ok_or_else(|| "native identity: import slot precedes executable image".to_owned())?;
    if slot_rva < iat_rva
        || slot_rva.checked_add(8).is_none_or(|end| end > iat_end_rva)
        || !(slot_rva - iat_rva).is_multiple_of(8)
    {
        return Err(
            "native identity: FF25 jump slot is outside the declared executable IAT".into(),
        );
    }
    let target = usize::try_from(u64::from_le_bytes(read_process_bytes::<8>(slot)?))
        .map_err(|_| "native identity: loaded import target exceeds pointer width".to_owned())?;
    if target == 0 || target == address {
        return Err("native identity: loaded import target is absent or self-referential".into());
    }
    Ok(ImportThunk {
        instruction,
        slot,
        slot_rva,
        target,
    })
}

fn module_path(module: *mut c_void) -> Result<PathBuf, String> {
    let mut filename = vec![0_u16; 32_768];
    // SAFETY: the caller supplies a currently loaded module, or NULL for this
    // executable. The buffer has exactly the stated number of writable units.
    let length =
        unsafe { GetModuleFileNameW(module, filename.as_mut_ptr(), filename.len() as u32) };
    if length == 0 {
        return Err(format!(
            "native identity: module path query failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    if length as usize >= filename.len() {
        return Err("native identity: module path exceeds the bounded UTF-16 buffer".into());
    }
    let filename = String::from_utf16(&filename[..length as usize])
        .map_err(|error| format!("native identity: module path is invalid UTF-16: {error}"))?;
    let path = PathBuf::from(filename);
    if !path.is_absolute() {
        return Err("native identity: loaded module path is not absolute".into());
    }
    Ok(path)
}

/// Identify the already loaded imports without constructing a native shifter.
/// Python and RubberBand must remain loaded throughout this test-only query.
pub(in crate::audio_engine) fn identify_loaded_native() -> Result<Value, String> {
    let imported_address = rubberband_live_shift as *const () as *const c_void;
    let import_module = module_from_address(imported_address)?;
    // SAFETY: NULL asks only for this process's already loaded executable.
    // This does not load a module or alter its reference count.
    let executable_module = unsafe { GetModuleHandleW(std::ptr::null()) };
    if executable_module.is_null() {
        return Err(format!(
            "native identity: executable module query failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    let thunk = if import_module == executable_module {
        Some(resolve_import_thunk(
            imported_address as usize,
            import_module,
        )?)
    } else {
        None
    };
    let resolved_address = thunk.map_or(imported_address, |thunk| thunk.target as *const c_void);
    let module = module_from_address(resolved_address)?;
    let loaded_path = module_path(module)?;
    if module == executable_module {
        return Err(format!(
            "native identity: resolved rubberband_live_shift target still belongs to the executable (static or unsupported binding): {}",
            loaded_path.display()
        ));
    }
    // SAFETY: module remains loaded and this fixed C string is NUL-terminated.
    // The address is compared only; no dynamically resolved function is called.
    let exported_address = unsafe { GetProcAddress(module, c"rubberband_live_shift".as_ptr()) };
    if exported_address.is_null() || !std::ptr::eq(exported_address.cast_const(), resolved_address)
    {
        return Err(format!(
            "native identity: rubberband_live_shift export does not match the resolved import target: {}",
            loaded_path.display()
        ));
    }

    let mut identity = c3_process_metrics::runtime_identity()?;
    let hashed_path = identity["rubberband_dll"]["path"].as_str().ok_or_else(|| {
        "native identity: existing runtime identity has no RubberBand path".to_owned()
    })?;
    if std::path::Path::new(hashed_path) != loaded_path.as_path() {
        return Err(format!(
            "native identity: function-bound RubberBand path {} differs from the hashed runtime path {hashed_path}",
            loaded_path.display()
        ));
    }
    if let Some(thunk) = thunk
        && (read_process_bytes::<6>(imported_address as usize)? != thunk.instruction
            || read_process_bytes::<8>(thunk.slot)? != (thunk.target as u64).to_le_bytes())
    {
        return Err("native identity: import thunk or IAT target changed during hashing".into());
    }
    identity["rubberband_function_binding"] = json!({
        "symbol": "rubberband_live_shift",
        "module_path": loaded_path,
        "resolution": "GetModuleHandleExW(FROM_ADDRESS|UNCHANGED_REFCOUNT)",
        "import_binding": if thunk.is_some() { "PE32+ x64 FF25 jump through validated executable IAT" } else { "direct function address" },
        "iat_slot_rva": thunk.map(|thunk| thunk.slot_rva),
        "iat_target_rechecked_after_hash": thunk.is_some(),
        "export_address_matches_import": std::ptr::eq(exported_address.cast_const(), imported_address),
        "export_address_matches_resolved_target": true,
        "hashed_path_matches_function_module": true,
        "scope": "bounded reads of this process's linked function, optional import thunk/IAT, and already loaded modules; no DLL loading or filesystem discovery",
    });
    identity["rubberband_library_version"] = json!({
        "file_version": null,
        "product_version": null,
        "status": "unavailable_until_exact_loaded_file_is_queried",
    });
    Ok(identity)
}
