"""Owned Windows worker trees, assigned while suspended before any child can run.

Uses documented Job Objects, Tool Help thread enumeration and ResumeThread APIs.
The unnamed non-inherited job permits no breakaway and kills its tree on last close.
"""

import ctypes
from ctypes import wintypes

CREATE_SUSPENDED = 0x00000004
_KILL_ON_JOB_CLOSE = 0x00002000
_JOB_EXTENDED_LIMITS = 9
_JOB_BASIC_ACCOUNTING = 1
_PROCESS_ASSIGN_ACCESS = 0x0101  # PROCESS_SET_QUOTA | PROCESS_TERMINATE
_THREAD_SUSPEND_RESUME = 0x0002
_SNAP_THREADS = 0x00000004
_INVALID_HANDLE = ctypes.c_void_p(-1).value
_NO_MORE_FILES = 18
_ACTIVE_PROCESS_LIMIT = 0x00000008
_MAX_JOB_PROCESSES = 64
_SYNCHRONIZE_QUERY = 0x00101000
_JOB_PROCESS_IDS = 3
_INVALID_PARAMETER = 87
_WAIT_TIMEOUT = 258


class _BasicLimits(ctypes.Structure):
    _fields_ = (
        ("process_user_time", ctypes.c_int64),
        ("job_user_time", ctypes.c_int64),
        ("flags", wintypes.DWORD),
        ("minimum_working_set", ctypes.c_size_t),
        ("maximum_working_set", ctypes.c_size_t),
        ("active_process_limit", wintypes.DWORD),
        ("affinity", ctypes.c_size_t),
        ("priority_class", wintypes.DWORD),
        ("scheduling_class", wintypes.DWORD),
    )


class _IoCounters(ctypes.Structure):
    _fields_ = tuple(
        (name, ctypes.c_uint64)
        for name in (
            "read_operations",
            "write_operations",
            "other_operations",
            "read_bytes",
            "write_bytes",
            "other_bytes",
        )
    )


class _ExtendedLimits(ctypes.Structure):
    _fields_ = (
        ("basic", _BasicLimits),
        ("io", _IoCounters),
        ("process_memory_limit", ctypes.c_size_t),
        ("job_memory_limit", ctypes.c_size_t),
        ("peak_process_memory", ctypes.c_size_t),
        ("peak_job_memory", ctypes.c_size_t),
    )


class _Accounting(ctypes.Structure):
    _fields_ = (
        ("user_time", ctypes.c_int64),
        ("kernel_time", ctypes.c_int64),
        ("period_user_time", ctypes.c_int64),
        ("period_kernel_time", ctypes.c_int64),
        ("page_faults", wintypes.DWORD),
        ("total_processes", wintypes.DWORD),
        ("active_processes", wintypes.DWORD),
        ("terminated_processes", wintypes.DWORD),
    )


class _ThreadEntry(ctypes.Structure):
    _fields_ = (
        ("size", wintypes.DWORD),
        ("usage", wintypes.DWORD),
        ("thread_id", wintypes.DWORD),
        ("process_id", wintypes.DWORD),
        ("base_priority", wintypes.LONG),
        ("delta_priority", wintypes.LONG),
        ("flags", wintypes.DWORD),
    )


class _ProcessIds(ctypes.Structure):
    _fields_ = (
        ("assigned", wintypes.DWORD),
        ("listed", wintypes.DWORD),
        ("process_ids", ctypes.c_size_t * _MAX_JOB_PROCESSES),
    )


def _kernel() -> ctypes.WinDLL:
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    signatures = (
        ("CreateJobObjectW", (ctypes.c_void_p, wintypes.LPCWSTR), wintypes.HANDLE),
        ("CloseHandle", (wintypes.HANDLE,), wintypes.BOOL),
        (
            "SetInformationJobObject",
            (wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p, wintypes.DWORD),
            wintypes.BOOL,
        ),
        (
            "QueryInformationJobObject",
            (wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p, wintypes.DWORD, ctypes.c_void_p),
            wintypes.BOOL,
        ),
        ("TerminateJobObject", (wintypes.HANDLE, wintypes.UINT), wintypes.BOOL),
        ("AssignProcessToJobObject", (wintypes.HANDLE, wintypes.HANDLE), wintypes.BOOL),
        ("OpenProcess", (wintypes.DWORD, wintypes.BOOL, wintypes.DWORD), wintypes.HANDLE),
        (
            "IsProcessInJob",
            (wintypes.HANDLE, wintypes.HANDLE, ctypes.POINTER(wintypes.BOOL)),
            wintypes.BOOL,
        ),
        ("WaitForSingleObject", (wintypes.HANDLE, wintypes.DWORD), wintypes.DWORD),
        ("OpenThread", (wintypes.DWORD, wintypes.BOOL, wintypes.DWORD), wintypes.HANDLE),
        ("ResumeThread", (wintypes.HANDLE,), wintypes.DWORD),
        ("CreateToolhelp32Snapshot", (wintypes.DWORD, wintypes.DWORD), wintypes.HANDLE),
        ("Thread32First", (wintypes.HANDLE, ctypes.POINTER(_ThreadEntry)), wintypes.BOOL),
        ("Thread32Next", (wintypes.HANDLE, ctypes.POINTER(_ThreadEntry)), wintypes.BOOL),
    )
    for name, arguments, result in signatures:
        function = getattr(kernel, name)
        function.argtypes = arguments
        function.restype = result
    return kernel


class WindowsJob:
    """Retain one unnamed process-tree job until all members have actually exited."""

    def __init__(self) -> None:
        self._kernel = _kernel()
        handle = self._kernel.CreateJobObjectW(None, None)
        if not handle:
            raise ctypes.WinError(ctypes.get_last_error())
        self._handle: int | None = int(handle)
        self._members: dict[int, int] = {}
        self._terminating = False
        try:
            self._set_process_limit(_MAX_JOB_PROCESSES)
        except OSError:
            self.close()
            raise

    def _set_process_limit(self, count: int) -> None:
        limits = _ExtendedLimits()
        limits.basic.flags = _KILL_ON_JOB_CLOSE | _ACTIVE_PROCESS_LIMIT
        limits.basic.active_process_limit = count
        if not self._kernel.SetInformationJobObject(
            self._handle, _JOB_EXTENDED_LIMITS, ctypes.byref(limits), ctypes.sizeof(limits)
        ):
            raise ctypes.WinError(ctypes.get_last_error())

    def assign_suspended(self, process_id: int) -> None:
        """Assign the newly created suspended process before resuming its initial thread."""
        process_handle = self._kernel.OpenProcess(_PROCESS_ASSIGN_ACCESS, 0, process_id)
        if not process_handle:
            raise ctypes.WinError(ctypes.get_last_error())
        try:
            if not self._kernel.AssignProcessToJobObject(self._handle, process_handle):
                raise ctypes.WinError(ctypes.get_last_error())
        finally:
            self._close_handle(int(process_handle))
        self.observe_members()
        self._resume_initial_thread(process_id)

    def terminate(self) -> None:
        """Request termination of every member, including descendants of an exited launcher."""
        if self._handle is not None and not self._terminating:
            # No live member can admit a child when the job's active-process limit
            # is one. Freeze child admission before capturing handles and killing.
            self._set_process_limit(1)
            self.observe_members()
            self._terminating = True
        if self._handle is not None and not self._kernel.TerminateJobObject(self._handle, 1):
            raise ctypes.WinError(ctypes.get_last_error())

    def active(self) -> bool:
        """Report kernel-owned active members, never infer retirement from the launcher's PID."""
        if self._handle is None:
            return False
        accounting = _Accounting()
        if not self._kernel.QueryInformationJobObject(
            self._handle,
            _JOB_BASIC_ACCOUNTING,
            ctypes.byref(accounting),
            ctypes.sizeof(accounting),
            None,
        ):
            raise ctypes.WinError(ctypes.get_last_error())
        return bool(accounting.active_processes) or self._members_active()

    def observe_members(self) -> None:
        """Retain member handles so asynchronous kernel exit cannot masquerade as retirement."""
        if self._handle is None:
            return
        self._discard_finished_members()
        members = _ProcessIds()
        if not self._kernel.QueryInformationJobObject(
            self._handle, _JOB_PROCESS_IDS, ctypes.byref(members), ctypes.sizeof(members), None
        ):
            raise ctypes.WinError(ctypes.get_last_error())
        for process_id in members.process_ids[: members.listed]:
            if process_id not in self._members:
                self._retain_member(int(process_id))

    def _retain_member(self, process_id: int) -> None:
        handle = self._kernel.OpenProcess(_SYNCHRONIZE_QUERY, 0, process_id)
        if not handle:
            error = ctypes.get_last_error()
            if error == _INVALID_PARAMETER:  # The process exited before OpenProcess.
                return
            raise ctypes.WinError(error)
        accepted = False
        try:
            member = wintypes.BOOL()
            if not self._kernel.IsProcessInJob(handle, self._handle, ctypes.byref(member)):
                raise ctypes.WinError(ctypes.get_last_error())
            if member.value:
                self._members[process_id] = int(handle)
                accepted = True
        finally:
            if not accepted:
                self._close_handle(int(handle))

    def _members_active(self) -> bool:
        return any(not self._member_finished(handle) for handle in self._members.values())

    def _discard_finished_members(self) -> None:
        for process_id, handle in tuple(self._members.items()):
            if self._member_finished(handle):
                self._close_handle(handle)
                del self._members[process_id]

    def _member_finished(self, handle: int) -> bool:
        result = self._kernel.WaitForSingleObject(handle, 0)
        if result == _WAIT_TIMEOUT:
            return False
        if result != 0:
            raise ctypes.WinError(ctypes.get_last_error())
        return True

    def close(self) -> None:
        """Close the sole owning handle; remaining members cannot outlive this job."""
        for process_id, handle in tuple(self._members.items()):
            self._close_handle(handle)
            del self._members[process_id]
        if self._handle is not None:
            self._close_handle(self._handle)
            self._handle = None

    def _close_handle(self, handle: int) -> None:
        if not self._kernel.CloseHandle(handle):
            raise ctypes.WinError(ctypes.get_last_error())

    def _resume_initial_thread(self, process_id: int) -> None:
        thread_ids = self._suspended_threads(process_id)
        if len(thread_ids) != 1:
            msg = "suspended worker did not have exactly one initial thread"
            raise OSError(msg)
        thread = self._kernel.OpenThread(_THREAD_SUSPEND_RESUME, 0, thread_ids[0])
        if not thread:
            raise ctypes.WinError(ctypes.get_last_error())
        try:
            previous_count = self._kernel.ResumeThread(thread)
            if previous_count == 0xFFFFFFFF:
                raise ctypes.WinError(ctypes.get_last_error())
            if previous_count != 1:
                msg = "worker initial thread had an unexpected suspend count"
                raise OSError(msg)
        finally:
            self._close_handle(int(thread))

    def _suspended_threads(self, process_id: int) -> list[int]:
        snapshot = self._kernel.CreateToolhelp32Snapshot(_SNAP_THREADS, 0)
        if snapshot == _INVALID_HANDLE:
            raise ctypes.WinError(ctypes.get_last_error())
        entry = _ThreadEntry()
        entry.size = ctypes.sizeof(entry)
        thread_ids = []
        try:
            present = self._kernel.Thread32First(snapshot, ctypes.byref(entry))
            while present:
                if entry.process_id == process_id:
                    thread_ids.append(int(entry.thread_id))
                entry.size = ctypes.sizeof(entry)
                present = self._kernel.Thread32Next(snapshot, ctypes.byref(entry))
            if ctypes.get_last_error() != _NO_MORE_FILES:
                raise ctypes.WinError(ctypes.get_last_error())
        finally:
            self._close_handle(int(snapshot))
        return thread_ids
