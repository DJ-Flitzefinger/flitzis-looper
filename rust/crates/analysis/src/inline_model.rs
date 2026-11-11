//! Same-handle Windows inline ONNX load with read-only local file custody.
use crate::trial_lifetime::{Context, OwnedFile, retain_first};
use std::{
    ffi::c_void,
    fs::OpenOptions,
    io,
    os::windows::{
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
    },
    path::Path,
    ptr::{self, NonNull},
    slice,
};

const FILE_SHARE_READ: u32 = 1;
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
const VOLUME_NAME_GUID: u32 = 1;
const FILE_NAME_OPENED: u32 = 8;
const PAGE_READONLY: u32 = 2;
const FILE_MAP_READ: u32 = 4;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetFinalPathNameByHandleW(file: *mut c_void, path: *mut u16, len: u32, flags: u32) -> u32;
    fn CreateFileMappingW(
        file: *mut c_void,
        attributes: *const c_void,
        protection: u32,
        max_high: u32,
        max_low: u32,
        name: *const u16,
    ) -> *mut c_void;
    fn MapViewOfFile(
        mapping: *mut c_void,
        access: u32,
        offset_high: u32,
        offset_low: u32,
        bytes: usize,
    ) -> *mut u8;
    fn UnmapViewOfFile(address: *const u8) -> i32;
    fn CloseHandle(handle: *mut c_void) -> i32;
}

/// Hold a local, non-reparse file of the exact extent against writes and deletion.
pub fn open(path: &Path, extent: usize, context: &Context) -> Result<OwnedFile, String> {
    if !path.is_absolute() || extent == 0 {
        return Err("expected an absolute path and nonempty ONNX graph".into());
    }
    let file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|error| format!("open inline ONNX graph: {error}"))?;
    let file = context.file(file, "onnx.file.close");
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file()
        || metadata.len() != extent as u64
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    {
        return Err("inline ONNX graph type or extent mismatch".into());
    }
    // SMB has no volume GUID. Check the opened object, with no pathname
    // lookup used later for hashing, mapping or ORT.
    // SAFETY: file is live and zero capacity requests the required length.
    let local = unsafe {
        GetFinalPathNameByHandleW(
            file.as_raw_handle(),
            ptr::null_mut(),
            0,
            VOLUME_NAME_GUID | FILE_NAME_OPENED,
        )
    };
    if local == 0 {
        return Err(format!(
            "inline ONNX graph requires a local volume: {}",
            io::Error::last_os_error()
        ));
    }
    Ok(file)
}

struct ReadView<'file> {
    address: Option<NonNull<u8>>,
    extent: usize,
    _file: &'file OwnedFile,
    context: &'file Context,
}

impl<'file> ReadView<'file> {
    fn new(file: &'file OwnedFile, extent: usize, context: &'file Context) -> Result<Self, String> {
        // SAFETY: this is a live read-only file, with validated exact extent.
        let mapping = unsafe {
            CreateFileMappingW(
                file.as_raw_handle(),
                ptr::null(),
                PAGE_READONLY,
                0,
                0,
                ptr::null(),
            )
        };
        if mapping.is_null() {
            return Err(format!(
                "map inline ONNX graph: {}",
                io::Error::last_os_error()
            ));
        }
        // SAFETY: unique live mapping handle with read access. The view
        // retains the section after its handle is closed.
        let mut result =
            NonNull::new(unsafe { MapViewOfFile(mapping, FILE_MAP_READ, 0, 0, extent) })
                .map(|address| Self {
                    address: Some(address),
                    extent,
                    _file: file,
                    context,
                })
                .ok_or_else(|| format!("view inline ONNX graph: {}", io::Error::last_os_error()));
        // SAFETY: consume the single handle once, on success and failure.
        if unsafe { CloseHandle(mapping) } == 0 {
            let failure = Err(format!(
                "close inline load mapping: {}",
                io::Error::last_os_error()
            ));
            context.record(failure.clone());
            retain_first(&mut result, failure);
        }
        result
    }

    fn bytes(&self) -> &[u8] {
        let address = self.address.expect("live view");
        // SAFETY: the exact-length read-only view lives through the borrow.
        // FILE_SHARE_READ excludes writers, including writable mappings,
        // while the borrowed local file remains open.
        unsafe { slice::from_raw_parts(address.as_ptr(), self.extent) }
    }

    fn finish(&mut self) -> Result<(), String> {
        let Some(address) = self.address.take() else {
            return Ok(());
        };
        // SAFETY: this is the base of our live view, consumed only once.
        let result = if unsafe { UnmapViewOfFile(address.as_ptr()) } == 0 {
            Err(format!(
                "unmap inline ONNX graph: {}",
                io::Error::last_os_error()
            ))
        } else {
            Ok(())
        };
        self.context.record(result.clone());
        result
    }
}

impl Drop for ReadView<'_> {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

/// Pass the exact opened bytes to a synchronous consumer, then unmap and close.
pub fn with_read_only<T>(
    path: &Path,
    extent: usize,
    context: &Context,
    use_bytes: impl FnOnce(&[u8]) -> Result<T, String>,
) -> Result<T, String> {
    let file = open(path, extent, context)?;
    let mut result = match ReadView::new(&file, extent, context) {
        Ok(mut view) => {
            let mut result = use_bytes(view.bytes());
            retain_first(&mut result, view.finish());
            result
        }
        Err(error) => Err(error),
    };
    retain_first(&mut result, file.complete());
    result
}
