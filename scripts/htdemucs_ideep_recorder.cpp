// Allocation observation for an isolated Windows x64 CPU-wheel producer probe.
// Build with /MD /DNDEBUG /D_ITERATOR_DEBUG_LEVEL=0 and the pinned iDeep and wheel headers.
#define WIN32_LEAN_AND_MEAN
#include <windows.h>

#include <array>
#include <bit>
#include <cstdint>
#include <malloc.h>

#include <ideep/abstract_types.hpp>

#if !defined(_M_X64) || !defined(_DLL) || _ITERATOR_DEBUG_LEVEL != 0
#error The iDeep recorder requires the Windows x64 release MSVC runtime ABI.
#endif

namespace {
constexpr size_t alignment = ideep::utils::allocator::tensor_memalignment;
static_assert(alignment == 4096);

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
uint64_t next_id = 1;
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
    void* ptr = ideep::utils::allocator::malloc(requested);
    const auto usable = ptr == nullptr ? 0 : _aligned_msize(ptr, alignment, 0);
    AcquireSRWLockExclusive(&lock);
    bool recorded = false;
    if (ptr != nullptr && usable != static_cast<size_t>(-1) && usable >= requested) {
        for (auto& entry : live) {
            if (entry.ptr == nullptr) {
                const Event event{next_id++, requested, usable, 1};
                entry = {ptr, event};
                append(event);
                recorded = true;
                break;
            }
        }
    }
    failed |= !recorded;
    ReleaseSRWLockExclusive(&lock);
    return ptr;
}

void release(void* ptr) {
    if (ptr != nullptr) {
        AcquireSRWLockExclusive(&lock);
        bool recorded = false;
        for (auto& entry : live) {
            if (entry.ptr == ptr) {
                Event event = entry.allocation;
                event.allocated = 0;
                append(event);
                entry = {};
                recorded = true;
                break;
            }
        }
        failed |= !recorded;
        ReleaseSRWLockExclusive(&lock);
    }
    ideep::utils::allocator::free(ptr);
}
} // namespace

extern "C" __declspec(dllexport) int install_ideep_recorder(const wchar_t* torch_cpu) {
    if (installed || torch_cpu == nullptr) {
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
    HMODULE recorder;
    // The singleton holds std::function targets in this image until process teardown.
    if (!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
                            std::bit_cast<LPCWSTR>(&install_ideep_recorder), &recorder)) {
        return 4;
    }
    using CpuEngine = ideep::engine& (*)();
    try {
        std::bit_cast<CpuEngine>(symbol)().set_allocator(allocate, release);
    } catch (...) {
        return 5;
    }
    installed = true;
    return 0;
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
