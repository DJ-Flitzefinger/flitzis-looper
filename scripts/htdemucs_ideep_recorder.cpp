// Allocation observation for an isolated Windows x64 CPU-wheel producer probe.
// Build with /MD /DNDEBUG /D_ITERATOR_DEBUG_LEVEL=0 and the pinned iDeep and wheel headers.
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <psapi.h>

#include <array>
#include <bit>
#include <cerrno>
#include <cstdint>
#include <malloc.h>

#include <ideep/abstract_types.hpp>

#if !defined(_M_X64) || !defined(_DLL) || _ITERATOR_DEBUG_LEVEL != 0
#error The iDeep recorder requires the Windows x64 release MSVC runtime ABI.
#endif

namespace {
constexpr size_t alignment = ideep::utils::allocator::tensor_memalignment;
static_assert(alignment == 4096);

enum class Kind : LONG64 {
    boundary = 1, allocate_begin, allocate_end, allocate_failed,
    free_begin, free_end, free_failed, invalid_stream, fault,
};

struct JournalRow {
    LONG64 kind; // Published last, after the remaining fields are visible.
    uint64_t thread;
    uint64_t phase;
    uint64_t id;
    uint64_t requested;
    uint64_t detail;
    uint64_t commit;
    uint64_t peak;
    uint64_t error;
};

constexpr size_t journal_capacity = 1024;
static_assert(sizeof(JournalRow) == 72);
JournalRow* journal = nullptr;
volatile LONG journal_cursor = 0;
volatile LONG current_phase = 0;
volatile LONG writing_fault = 0;
bool page_backed = false;

uint64_t phase() {
    return static_cast<uint64_t>(InterlockedCompareExchange(&current_phase, 0, 0));
}

bool publish(Kind kind, JournalRow row) {
    const auto slot = static_cast<size_t>(InterlockedIncrement(&journal_cursor) - 1);
    if (slot >= journal_capacity) {
        return false;
    }
    journal[slot] = row;
    InterlockedExchange64(&journal[slot].kind, static_cast<LONG64>(kind));
    return true;
}

[[noreturn]] void stop_probe(DWORD code) {
    TerminateProcess(GetCurrentProcess(), code);
    __fastfail(FAST_FAIL_FATAL_APP_EXIT);
}

void sample(Kind kind, uint64_t id = 0, uint64_t requested = 0,
            uint64_t detail = 0, uint64_t error = 0) {
    PROCESS_MEMORY_COUNTERS_EX memory{};
    memory.cb = sizeof(memory);
    if (!K32GetProcessMemoryInfo(GetCurrentProcess(),
                                reinterpret_cast<PROCESS_MEMORY_COUNTERS*>(&memory), memory.cb)) {
        // Distinguish an unavailable memory sample from the operation's error.
        memory.PrivateUsage = UINT64_MAX;
        memory.PeakPagefileUsage = GetLastError();
    }
    if (!publish(kind, {0, GetCurrentThreadId(), phase(), id, requested, detail,
                        memory.PrivateUsage, memory.PeakPagefileUsage, error})) {
        stop_probe(ERROR_BUFFER_OVERFLOW);
    }
}

LONG CALLBACK observe_exception(EXCEPTION_POINTERS* pointers) {
    const auto* record = pointers->ExceptionRecord;
    const auto active = phase();
    if (active != 0 &&
        (record->ExceptionCode == EXCEPTION_ACCESS_VIOLATION ||
         record->ExceptionCode == EXCEPTION_IN_PAGE_ERROR) &&
        InterlockedCompareExchange(&writing_fault, 1, 0) == 0) {
        const bool has_address = record->NumberParameters >= 2;
        // Use only the existing mapping and atomics. Preserve exception dispatch.
        __try {
            publish(Kind::fault, {0, GetCurrentThreadId(), active,
                                 reinterpret_cast<uintptr_t>(record->ExceptionAddress),
                                 has_address ? record->ExceptionInformation[1] : 0,
                                 has_address ? record->ExceptionInformation[0] : UINT64_MAX,
                                 UINT64_MAX, 0, record->ExceptionCode});
        } __except (EXCEPTION_EXECUTE_HANDLER) {
            // A fault in the mapped journal must not replace the original exception.
        }
        InterlockedExchange(&writing_fault, 0);
    }
    return EXCEPTION_CONTINUE_SEARCH;
}

bool open_journal(const wchar_t* path) {
    const auto file = CreateFileW(path, GENERIC_READ | GENERIC_WRITE,
                                  FILE_SHARE_READ | FILE_SHARE_WRITE, nullptr, CREATE_NEW,
                                  FILE_ATTRIBUTE_NORMAL, nullptr);
    if (file == INVALID_HANDLE_VALUE) {
        return false;
    }
    constexpr DWORD bytes = sizeof(JournalRow) * journal_capacity;
    const auto mapping = CreateFileMappingW(file, nullptr, PAGE_READWRITE, 0, bytes, nullptr);
    CloseHandle(file);
    if (mapping == nullptr) {
        return false;
    }
    journal = static_cast<JournalRow*>(MapViewOfFile(mapping, FILE_MAP_WRITE, 0, 0, bytes));
    CloseHandle(mapping);
    if (journal == nullptr) {
        return false;
    }
    ZeroMemory(journal, bytes);
    return true;
}


struct Event {
    uint64_t id;
    uint64_t requested;
    uint64_t usable;
    uint64_t allocated;
};

struct Live {
    void* ptr;
    Event allocation;
};

SRWLOCK lock = SRWLOCK_INIT;
std::array<Live, 128> live{};
std::array<Event, 512> events{};
size_t count = 0;
volatile LONG64 next_id = 0;
bool failed = false;
bool installed = false;

void append(Event event) {
    if (count < events.size()) {
        events[count++] = event;
    } else {
        failed = true;
    }
}

void* allocate(size_t requested) {
    const auto id = static_cast<uint64_t>(InterlockedIncrement64(&next_id));
    const bool pages = page_backed && requested != 0;
    sample(Kind::allocate_begin, id, requested, pages);
    void* ptr = pages ? VirtualAlloc(nullptr, requested, MEM_RESERVE | MEM_COMMIT, PAGE_READWRITE)
                      : ideep::utils::allocator::malloc(requested);
    if (ptr == nullptr) {
        const auto error = pages ? GetLastError() : static_cast<DWORD>(errno);
        sample(Kind::allocate_failed, id, requested, pages, error);
        // iDeep otherwise forwards this failed nonzero allocation as a memory handle.
        if (requested != 0) {
            stop_probe(ERROR_NOT_ENOUGH_MEMORY);
        }
        return nullptr;
    }
    const auto usable = pages ? requested : _aligned_msize(ptr, alignment, 0);
    sample(Kind::allocate_end, id, requested, usable);
    AcquireSRWLockExclusive(&lock);
    if (usable != static_cast<size_t>(-1) && usable >= requested) {
        for (auto& entry : live) {
            if (entry.ptr == nullptr) {
                const Event event{id, requested, usable, 1};
                entry = {ptr, event};
                append(event);
                ReleaseSRWLockExclusive(&lock);
                return ptr;
            }
        }
    }
    sample(Kind::invalid_stream, id, requested, usable);
    stop_probe(ERROR_INVALID_DATA);
}

void release(void* ptr) {
    if (ptr == nullptr) {
        return;
    }
    Event event{};
    AcquireSRWLockExclusive(&lock);
    for (auto& entry : live) {
        if (entry.ptr == ptr) {
            event = entry.allocation;
            entry = {};
            break;
        }
    }
    ReleaseSRWLockExclusive(&lock);
    if (event.id == 0) {
        sample(Kind::invalid_stream);
        stop_probe(ERROR_INVALID_DATA);
    }

    const bool pages = page_backed && event.requested != 0;
    const auto address = reinterpret_cast<uintptr_t>(ptr);
    sample(Kind::free_begin, event.id, event.requested, pages);
    if (pages) {
        if (!VirtualFree(ptr, 0, MEM_RELEASE)) {
            const auto error = GetLastError();
            sample(Kind::free_failed, event.id, event.requested, pages, error);
            stop_probe(ERROR_INVALID_ADDRESS);
        }
    } else {
        ideep::utils::allocator::free(ptr);
    }
    MEMORY_BASIC_INFORMATION memory{};
    const auto queried = VirtualQuery(reinterpret_cast<void*>(address), &memory, sizeof(memory));
    const DWORD query_error = queried ? 0 : GetLastError();
    sample(Kind::free_end, event.id, event.requested, memory.State, query_error);
    event.allocated = 0;
    AcquireSRWLockExclusive(&lock);
    append(event);
    ReleaseSRWLockExclusive(&lock);
}
} // namespace

extern "C" __declspec(dllexport) int install_ideep_recorder(
    const wchar_t* torch_cpu, const wchar_t* journal_path, int use_pages) {
    if (installed || torch_cpu == nullptr || journal_path == nullptr) {
        return 1;
    }
    const HMODULE torch = GetModuleHandleW(torch_cpu);
    if (torch == nullptr) {
        return 2;
    }
    const auto symbol = GetProcAddress(torch, "?cpu_engine@engine@ideep@@SAAEAU12@XZ");
    if (symbol == nullptr) {
        return 3;
    }
    SYSTEM_INFO system{};
    GetSystemInfo(&system);
    if (use_pages && (system.dwPageSize != alignment || system.dwAllocationGranularity % alignment)) {
        return 4;
    }
    HMODULE recorder;
    // The singleton holds std::function targets in this image until process teardown.
    if (!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
                            std::bit_cast<LPCWSTR>(&install_ideep_recorder), &recorder)) {
        return 5;
    }
    if (!open_journal(journal_path) || AddVectoredExceptionHandler(1, observe_exception) == nullptr) {
        return 6;
    }
    page_backed = use_pages != 0;
    using CpuEngine = ideep::engine& (*)();
    try {
        std::bit_cast<CpuEngine>(symbol)().set_allocator(allocate, release);
    } catch (...) {
        return 7;
    }
    installed = true;
    return 0;
}

extern "C" __declspec(dllexport) void mark_ideep_phase(LONG value) {
    InterlockedExchange(&current_phase, value);
    sample(Kind::boundary);
}

extern "C" __declspec(dllexport) int read_ideep_events(Event* output, size_t capacity) {
    AcquireSRWLockShared(&lock);
    int result = -1;
    if (installed && !failed && output != nullptr && capacity >= count) {
        for (size_t index = 0; index < count; ++index) {
            output[index] = events[index];
        }
        result = static_cast<int>(count);
    }
    ReleaseSRWLockShared(&lock);
    return result;
}
