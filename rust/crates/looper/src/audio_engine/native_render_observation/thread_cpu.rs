//! Opt-in observations on the original test render thread, never a sampler.

use std::ffi::c_void;
use std::os::windows::io::{AsRawHandle, OwnedHandle};
use std::time::Instant;

#[repr(C)]
#[derive(Default)]
struct FileTime {
    low: u32,
    high: u32,
}

impl FileTime {
    fn ticks(self) -> u64 {
        (u64::from(self.high) << 32) | u64::from(self.low)
    }
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentThread() -> *mut c_void;
    fn GetCurrentThreadId() -> u32;
    fn GetThreadId(thread: *mut c_void) -> u32;
    fn GetThreadTimes(
        thread: *mut c_void,
        creation: *mut FileTime,
        exit: *mut FileTime,
        kernel: *mut FileTime,
        user: *mut FileTime,
    ) -> i32;
    fn QueryThreadCycleTime(thread: *mut c_void, cycles: *mut u64) -> i32;
}

/// Raw accumulated counters. FILETIME units do not imply 100 ns resolution.
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub(crate) struct Point {
    pub thread_id: u32,
    pub birth_100ns: u64,
    pub kernel_100ns: u64,
    pub user_100ns: u64,
    pub cycles: u64,
    pub query_wall_ns: u128,
}

impl Point {
    fn current() -> Self {
        // SAFETY: a pseudo handle is always valid on this calling thread.
        Self::read(unsafe { GetCurrentThread() }, unsafe {
            GetCurrentThreadId()
        })
    }

    pub(crate) fn worker(handle: &OwnedHandle) -> Self {
        // SAFETY: the owned duplicate retains the exact worker kernel object.
        Self::read(handle.as_raw_handle(), unsafe {
            GetThreadId(handle.as_raw_handle())
        })
    }

    fn read(thread: *mut c_void, thread_id: u32) -> Self {
        let begin = Instant::now();
        let (mut creation, mut exit, mut kernel, mut user) = (
            FileTime::default(),
            FileTime::default(),
            FileTime::default(),
            FileTime::default(),
        );
        let mut cycles = 0;
        // SAFETY: initialized outputs and a valid current-thread pseudo handle
        // or retained worker handle; no extra thread, priority or affinity change.
        unsafe {
            let times_ok = GetThreadTimes(thread, &mut creation, &mut exit, &mut kernel, &mut user);
            assert_ne!(
                times_ok,
                0,
                "GetThreadTimes: {}",
                std::io::Error::last_os_error()
            );
            let cycles_ok = QueryThreadCycleTime(thread, &mut cycles);
            assert_ne!(
                cycles_ok,
                0,
                "QueryThreadCycleTime: {}",
                std::io::Error::last_os_error()
            );
        };
        assert_ne!(thread_id, 0);
        Self {
            thread_id,
            birth_100ns: creation.ticks(),
            kernel_100ns: kernel.ticks(),
            user_100ns: user.ticks(),
            cycles,
            query_wall_ns: begin.elapsed().as_nanos(),
        }
    }
}

/// Both snapshots and their query skew are retained; cycles are never time.
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub(crate) struct Span {
    pub before: Point,
    pub after: Point,
    pub wall_ns: u128,
}

pub(crate) struct Timer {
    begin: Instant,
    before: Point,
}

impl Timer {
    pub(crate) fn begin() -> Self {
        Self {
            begin: Instant::now(),
            before: Point::current(),
        }
    }

    pub(crate) fn finish(self) -> Span {
        let after = Point::current();
        assert_eq!(
            (self.before.thread_id, self.before.birth_100ns),
            (after.thread_id, after.birth_100ns)
        );
        assert!(after.cycles >= self.before.cycles);
        assert!(after.kernel_100ns >= self.before.kernel_100ns);
        assert!(after.user_100ns >= self.before.user_100ns);
        Span {
            before: self.before,
            after,
            wall_ns: self.begin.elapsed().as_nanos(),
        }
    }
}

/// The declared matrix has at most one shift per PAD in a 96-frame callback.
#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
pub(crate) struct Shifts {
    pub spans: [Option<Span>; 6],
    pub count: usize,
}

impl Shifts {
    pub(super) fn push(&mut self, span: Span) {
        assert!(
            self.count < self.spans.len(),
            "bounded shift observation exhausted"
        );
        self.spans[self.count] = Some(span);
        self.count += 1;
    }
}
