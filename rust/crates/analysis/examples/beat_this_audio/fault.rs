//! Narrow standalone ORT 1.24.4 arena-limit diagnostic, never App policy.
use std::collections::HashMap;

#[derive(Default)]
pub struct Evidence {
    cuda_creation: Option<String>,
    last_extend: HashMap<String, Option<u64>>,
    invalid: bool,
}

impl Evidence {
    pub fn observe(&mut self, thread: &str, location: &str, message: &str) {
        if message.len() > 16384 || self.last_extend.len() > 32 {
            self.invalid = true;
            return;
        }
        if !location.contains("bfc_arena.cc:") {
            if message.starts_with("Creating BFCArena for ")
                || message.starts_with("Extending BFCArena for ")
            {
                self.invalid = true;
            }
            return;
        }
        if message.starts_with("Creating BFCArena for Cuda with following configs:") {
            if self.cuda_creation.is_some()
                || !message.contains(" memory limit: 1 arena_extend_strategy:")
            {
                self.invalid = true;
            }
            self.cuda_creation = Some(message.to_owned());
            self.last_extend.clear();
        }
        if let Some(details) = message.strip_prefix("Extending BFCArena for ") {
            // EVERY arena's extend invalidates earlier evidence on this thread.
            // In particular CudaPinned is host memory and cannot match Cuda.
            let rounded = details.strip_prefix("Cuda. bin_num:").and_then(|s| {
                let (bin, s) = s.split_once(" (requested) num_bytes: ")?;
                bin.parse::<u64>().ok()?;
                let (requested, rounded) = s.split_once(" (actual) rounded_bytes:")?;
                let requested = requested.parse::<u64>().ok()?;
                let rounded = rounded.parse::<u64>().ok()?;
                (requested > 0
                    && rounded > 0
                    && rounded % 256 == 0
                    && requested.checked_add(255)? / 256 * 256 == rounded)
                    .then_some(rounded)
            });
            self.last_extend.insert(thread.to_owned(), rounded);
        }
    }

    pub fn fault(&self, thread: &str, error: &str) -> Option<u64> {
        if self.invalid
            || self.cuda_creation.is_none()
            || self.last_extend.len() > 32
            || !error.contains("bfc_arena.cc:")
            || !error.contains("BFCArena::AllocateRawInternal")
        {
            return None;
        }
        let rounded = self.last_extend.get(thread).copied().flatten()?;
        let (_, suffix) =
            error.rsplit_once("Available memory of 0 is smaller than requested bytes of ")?;
        (suffix.trim().parse::<u64>().ok()? == rounded).then_some(rounded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const CREATE: &str = "Creating BFCArena for Cuda with following configs: initial_chunk_size_bytes: 1048576 memory limit: 1 arena_extend_strategy: 0";
    const EXTEND: &str = "Extending BFCArena for Cuda. bin_num:0 (requested) num_bytes: 4 (actual) rounded_bytes:256";
    const ERROR: &str = "Exception during loading: bfc_arena.cc:329 onnxruntime::BFCArena::AllocateRawInternal Available memory of 0 is smaller than requested bytes of 256";
    fn evidence() -> Evidence {
        let mut e = Evidence::default();
        e.observe("main", "bfc_arena.cc:24 BFCArena", CREATE);
        e.observe("main", "bfc_arena.cc:306 AllocateRawInternal", EXTEND);
        e
    }
    #[test]
    fn exact_device_arena_refusal_requires_all_independent_observations() {
        assert_eq!(evidence().fault("main", ERROR), Some(256));
        assert_eq!(evidence().fault("other", ERROR), None);
        for error in [
            "bad allocation",
            "CUDA failure",
            "BFCArena allocation failed",
            "Available memory of 0 is smaller than requested bytes of 256",
        ] {
            assert_eq!(evidence().fault("main", error), None);
        }
        for allocator in ["Cpu", "CudaPinned", "CudaExtra"] {
            let mut e = evidence();
            e.observe(
                "main",
                "bfc_arena.cc:306",
                &EXTEND.replace("Cuda.", &format!("{allocator}.")),
            );
            assert_eq!(e.fault("main", ERROR), None);
        }
        for (requested, rounded) in [(0, 256), (1, 0), (4, 255), (4, 512), (257, 256)] {
            let mut e = evidence();
            e.observe("main", "bfc_arena.cc:306", &format!("Extending BFCArena for Cuda. bin_num:0 (requested) num_bytes: {requested} (actual) rounded_bytes:{rounded}"));
            assert_eq!(e.fault("main", ERROR), None);
        }
        assert_eq!(
            evidence().fault("main", &ERROR.replace("of 256", "of 512")),
            None
        );
        let mut e = evidence();
        e.observe("main", "bfc_arena.cc:24", CREATE);
        assert_eq!(e.fault("main", ERROR), None);
        let mut e = Evidence::default();
        e.observe("main", "bfc_arena.cc:306", EXTEND);
        assert_eq!(e.fault("main", ERROR), None);
    }
}
