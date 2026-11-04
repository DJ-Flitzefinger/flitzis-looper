//! Private per-Engine acquisition adapter; all render and close paths stay real.

use super::{NativeSession, checked};
use std::ptr::{self, NonNull};
use std::sync::Arc;

#[repr(C)]
struct State {
    _private: [u8; 0],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
pub(in crate::audio_engine) struct DeviceInfo {
    pub rate: f64,
    pub created: u64,
    pub released: u64,
    pub active: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
pub(in crate::audio_engine) struct CallbackMeasurement {
    pub callback_ticks: u64,
    pub qpc_frequency: u64,
    pub output_ready_calls: u64,
    pub frames: u32,
    pub guards_intact: u32,
    pub left_peak: f64,
    pub right_peak: f64,
}

#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
pub(in crate::audio_engine) struct PadObservation {
    pub active: bool,
    pub generation: u64,
    pub frame: usize,
    pub fraction: f64,
    pub post_dsp_peak: f32,
    pub key_lock: bool,
    pub stem_mode: u8,
    pub stem_mask: u8,
    pub source_pcm: usize,
    pub source_samples: usize,
    pub native_state: usize,
    pub native_block_size: usize,
    pub native_start_delay: usize,
    pub input_fifo_capacity: usize,
    pub output_fifo_capacity: usize,
    pub productive_fed_frames: u64,
    pub wet_current_source_output: bool,
    pub wet_failed: bool,
}

#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
pub(in crate::audio_engine) struct TickObservation {
    pub callback: CallbackMeasurement,
    pub native: crate::audio_engine::native_render_observation::Observation,
    pub output_frame: u64,
    pub pads: [PadObservation; 6],
}

const _: () = {
    assert!(std::mem::size_of::<DeviceInfo>() == 32);
    assert!(std::mem::size_of::<CallbackMeasurement>() == 48);
};

unsafe extern "C" {
    fn fl_asio_test_state_new(out: *mut *mut State) -> i32;
    fn fl_asio_test_state_delete(state: *mut State) -> i32;
    fn fl_asio_test_state_info(state: *mut State, out: *mut DeviceInfo) -> i32;
    fn fl_asio_test_fail_start(state: *mut State, session_number: u64) -> i32;
    fn fl_asio_test_fail_tick(state: *mut State, session_number: u64) -> i32;
    fn fl_asio_test_new(state: *mut State, out: *mut *mut NativeSession) -> i32;
    fn fl_asio_test_tick(
        session: *mut NativeSession,
        state: *mut State,
        system_ns: u64,
        out: *mut CallbackMeasurement,
    ) -> i32;
}

#[derive(Debug)]
pub(in crate::audio_engine) struct Factory {
    state: NonNull<State>,
}

// The C++ state contains only atomics. Arc retains it through every original
// owner thread; driver objects themselves never leave that control thread.
unsafe impl Send for Factory {}
unsafe impl Sync for Factory {}

impl Factory {
    pub fn new() -> Result<Arc<Self>, String> {
        let mut state = ptr::null_mut();
        checked("create private test device state", unsafe {
            fl_asio_test_state_new(&mut state)
        })?;
        Ok(Arc::new(Self {
            state: NonNull::new(state).ok_or("private device state is null")?,
        }))
    }

    pub fn info(&self) -> Result<DeviceInfo, String> {
        let mut info = DeviceInfo::default();
        checked("query private device state", unsafe {
            fl_asio_test_state_info(self.state.as_ptr(), &mut info)
        })?;
        Ok(info)
    }

    pub fn fail_start_of_session(&self, session_number: u64) -> Result<(), String> {
        checked("inject one private Start failure", unsafe {
            fl_asio_test_fail_start(self.state.as_ptr(), session_number)
        })
    }

    pub fn fail_callback_of_session(&self, session_number: u64) -> Result<(), String> {
        checked("inject one private callback failure", unsafe {
            fl_asio_test_fail_tick(self.state.as_ptr(), session_number)
        })
    }

    pub(super) fn acquire(&self, out: &mut *mut NativeSession) -> Result<(), String> {
        checked("acquire private test driver", unsafe {
            fl_asio_test_new(self.state.as_ptr(), out)
        })
    }

    pub(super) fn tick(
        &self,
        session: *mut NativeSession,
        system_ns: u64,
    ) -> Result<CallbackMeasurement, String> {
        let mut measurement = CallbackMeasurement::default();
        checked("drive private original callback", unsafe {
            fl_asio_test_tick(session, self.state.as_ptr(), system_ns, &mut measurement)
        })?;
        Ok(measurement)
    }
}

impl Drop for Factory {
    fn drop(&mut self) {
        assert_eq!(
            unsafe { fl_asio_test_state_delete(self.state.as_ptr()) },
            0,
            "private device state outlived by an actual driver owner"
        );
    }
}
