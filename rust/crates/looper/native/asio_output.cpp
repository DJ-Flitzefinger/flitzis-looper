#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <objbase.h>
#include <atomic>
#include <cmath>
#include <cstddef>
#include <cstring>
#include <new>
#include "iasiodrv.h"
#include "asio_output.h"

static_assert(sizeof(long) == 4 && sizeof(ASIOBool) == 4);
static_assert(sizeof(ASIOSamples) == 8 && sizeof(ASIOTimeStamp) == 8);
static_assert(sizeof(AsioTimeInfo) == 48 && offsetof(AsioTimeInfo, flags) == 32);
static_assert(sizeof(ASIOTime) == 148 && offsetof(ASIOTime, timeInfo) == 16);
static_assert(sizeof(FlAsioInfo) == 48 && offsetof(FlAsioInfo, output_ready) == 40);
static_assert(sizeof(FlAsioTime) == 40 && offsetof(FlAsioTime, flags) == 24);
static_assert(sizeof(FlAsioChannel) == 44);
static_assert(sizeof(FlAsioStatus) == 32 && offsetof(FlAsioStatus, last_notified_rate) == 16);
static_assert(sizeof(FlAsioRateTransaction) == 64);
static_assert(std::atomic<void*>::is_always_lock_free);
static_assert(std::atomic<uint32_t>::is_always_lock_free);
static_assert(std::atomic<uint64_t>::is_always_lock_free);

namespace {
constexpr int32_t invalid_state = -2000;
constexpr int32_t busy_session = -2001;
constexpr int32_t callback_reentry = -2002;
constexpr int32_t driver_exception = -2003;
constexpr int32_t invalid_geometry = -2004;
constexpr int32_t invalid_callback_index = -2005;
constexpr uint32_t reset_required = 1;
constexpr uint32_t rate_changed = 2;
constexpr uint32_t resync_required = 4;
constexpr uint32_t latencies_changed = 8;
constexpr uint32_t overload = 16;
constexpr uint32_t clock_lost = 32;
constexpr uint32_t same_rate_notice = 64;
std::atomic<bool> session_claimed{false};
std::atomic<FlAsioSession*> current{nullptr};
// Entry precedes loading current, so detachment cannot race a still-unregistered
// reader. This storage outlives every session and is bounded to one driver.
std::atomic<uint32_t> callback_entries{0};

int sample_bytes(long type) noexcept {
    switch (type) {
    case ASIOSTInt16MSB: case ASIOSTInt16LSB: return 2;
    case ASIOSTInt24MSB: case ASIOSTInt24LSB: return 3;
    case ASIOSTFloat64MSB: case ASIOSTFloat64LSB: return 8;
    case ASIOSTInt32MSB: case ASIOSTInt32LSB:
    case ASIOSTFloat32MSB: case ASIOSTFloat32LSB:
    case ASIOSTInt32MSB16: case ASIOSTInt32MSB18:
    case ASIOSTInt32MSB20: case ASIOSTInt32MSB24:
    case ASIOSTInt32LSB16: case ASIOSTInt32LSB18:
    case ASIOSTInt32LSB20: case ASIOSTInt32LSB24: return 4;
    default: return 0;
    }
}

bool legal_frames(long frames, const FlAsioInfo& info) noexcept {
    if (frames < info.min_frames || frames > info.max_frames || frames <= 0)
        return false;
    if (info.granularity == -1) {
        const auto min = info.min_frames;
        if (min <= 0 || frames % min != 0) return false;
        const auto ratio = frames / min;
        return (ratio & (ratio - 1)) == 0;
    }
    if (info.granularity > 0)
        return (frames - info.min_frames) % info.granularity == 0;
    // Zero has no reliable arithmetic sequence. A coherent preferred value is
    // usable, including fixed-size drivers; arbitrary intermediate sizes are not.
    return info.granularity == 0 && frames == info.preferred_frames;
}

struct Entry {
    FlAsioSession* session;
    Entry() noexcept : session(nullptr) {
        callback_entries.fetch_add(1, std::memory_order_seq_cst);
        session = current.load(std::memory_order_seq_cst);
    }
    ~Entry() { callback_entries.fetch_sub(1, std::memory_order_seq_cst); }
};
}

#ifdef FLITZIS_ASIO_CONTRACT_TESTS
struct FlAsioTestState;
#endif
struct FlAsioSession {
    IASIO* driver = nullptr;
    HWND window = nullptr;
    DWORD thread = GetCurrentThreadId();
    bool com = false;
    bool closed = false;
    bool buffers = false;
    bool start_attempted = false;
    bool detached = false;
    bool ready = false;
    std::atomic<bool> prepared{false};
    long frames = 0;
    double configured_rate = 0;
    ASIOBufferInfo planes[2]{};
    ASIOChannelInfo channels[2]{};
    ASIOCallbacks callbacks{};
    void* render_context = nullptr;
    FlAsioRender render = nullptr;
    FlAsioInvalidate invalidate = nullptr;
    std::atomic_flag rendering = ATOMIC_FLAG_INIT;
    std::atomic<uint32_t> notifications{0};
    std::atomic<int32_t> callback_error{0};
    std::atomic<uint32_t> latency_revision{0};
    std::atomic<uint64_t> notified_rate_bits{0};
    std::atomic<uint64_t> rate_notice_count{0};
    FlAsioRateTransaction rate{}; // owner thread only; notices use atomics above
#ifdef FLITZIS_ASIO_CONTRACT_TESTS
    FlAsioTestState* test_state = nullptr;
#endif
};

namespace {
bool control_owner(FlAsioSession* s) noexcept {
    return s && s->thread == GetCurrentThreadId() && !s->closed;
}

int32_t initialize_selected_driver(FlAsioSession* s, void* system_handle) {
    if (!s->driver->init(system_handle)) return ASE_NotPresent;
    const auto result = s->driver->getSampleRate(&s->rate.original_rate);
    if (result != ASE_OK) return result;
    if (!std::isfinite(s->rate.original_rate) || s->rate.original_rate <= 0) return invalid_geometry;
    s->rate.flags = 1;
    return ASE_OK;
}

void abort_rate_transaction(FlAsioSession* s) noexcept {
    auto& rate = s->rate;
    if (rate.flags & 4) { rate.disposition = 1; return; }
    if (!(rate.flags & 2)) return;
    rate.disposition = 4;
    if (!(rate.flags & 1) || !s->driver) return;
    if (s->notifications.load(std::memory_order_acquire)
        & (reset_required | rate_changed | resync_required | clock_lost)) {
        rate.flags |= 8; rate.disposition = 3; return;
    }
    try {
        rate.restore_query_status = s->driver->getSampleRate(&rate.restore_before_rate);
        if (rate.restore_query_status != ASE_OK || !std::isfinite(rate.restore_before_rate)
            || rate.restore_before_rate <= 0) return;
        if (rate.restore_before_rate == rate.original_rate) {
            rate.restore_after_rate = rate.original_rate; rate.disposition = 0; return;
        }
        // IASIO has no compare-and-set. Preserve an external/unattributed rate
        // rather than overwrite it. The remaining query/set race is reported as
        // guarded best-effort, never as an exclusive driver-global transaction.
        if (rate.query_status != ASE_OK || rate.observed_rate != rate.target_rate
            || rate.restore_before_rate != rate.observed_rate) {
            rate.disposition = 3; return;
        }
        try { rate.restore_set_status = s->driver->setSampleRate(rate.original_rate); }
        catch (...) { rate.restore_set_status = driver_exception; }
        // A throwing setter can have effects. Observe the actual after-rate
        // independently even when the setter did not return successfully.
        try { rate.restore_query_status = s->driver->getSampleRate(&rate.restore_after_rate); }
        catch (...) { rate.restore_query_status = driver_exception; }
        rate.disposition = rate.restore_set_status == ASE_OK
            && rate.restore_query_status == ASE_OK
            && rate.restore_after_rate == rate.original_rate ? 2 : 5;
    } catch (...) { rate.restore_set_status = driver_exception; rate.disposition = 5; }
}

void fault(FlAsioSession* s, int32_t value) noexcept {
    int32_t empty = 0;
    s->callback_error.compare_exchange_strong(empty, value, std::memory_order_relaxed);
    if (s->invalidate) s->invalidate(s->render_context, 64, 0);
}

void clear_planes(FlAsioSession* s, long index) noexcept {
    for (int channel = 0; channel < 2; ++channel) {
        std::memset(s->planes[channel].buffers[index], 0,
                    static_cast<size_t>(s->frames) * sample_bytes(s->channels[channel].type));
    }
}

void process(ASIOTime* time, long index, ASIOBool direct) noexcept {
    Entry entry;
    auto* s = entry.session;
    if (!s) return;
    if (!s->prepared.load(std::memory_order_acquire)) {
        fault(s, invalid_state);
        return;
    }
    if (s->rendering.test_and_set(std::memory_order_acquire)) {
        // Do not touch planes or the Rust context while another callback owns
        // them. Control observes the terminal fault and stops the original driver.
        fault(s, callback_reentry);
        return;
    }
    if (index < 0 || index > 1) {
        fault(s, invalid_callback_index);
    } else {
        try {
            clear_planes(s, index);
            FlAsioTime normalized{};
            normalized.direct_process = direct;
            ASIOTime legacy_time{};
            if (!time) {
                // ASIO1's bufferSwitch supplies no timeInfo structure. Its
                // specified getSamplePosition pair is the same driver clock,
                // not a host-clock estimate or a second observer/device.
                const auto result = s->driver->getSamplePosition(
                    &legacy_time.timeInfo.samplePosition, &legacy_time.timeInfo.systemTime);
                if (result == ASE_OK) {
                    legacy_time.timeInfo.sampleRate = s->configured_rate;
                    legacy_time.timeInfo.flags = kSystemTimeValid | kSamplePositionValid | kSampleRateValid;
                    time = &legacy_time;
                } else if (result != ASE_NoClock) { fault(s, result); }
            }
            if (time) {
                    const auto& info = time->timeInfo;
                    normalized.system_ns = (static_cast<uint64_t>(info.systemTime.hi) << 32)
                        | info.systemTime.lo;
                    normalized.sample_position = (static_cast<uint64_t>(info.samplePosition.hi) << 32)
                        | info.samplePosition.lo;
                    normalized.sample_rate = info.sampleRate;
                    normalized.flags = info.flags;
                    const bool rate_valid = (info.flags & kSampleRateValid) != 0;
                    const bool same_rate = rate_valid && std::isfinite(info.sampleRate)
                        && info.sampleRate == s->configured_rate;
                    if ((info.flags & kClockSourceChanged)
                        || ((info.flags & kSampleRateChanged) && !same_rate)
                        || (rate_valid && !same_rate))
                    {
                        s->notifications.fetch_or(rate_changed, std::memory_order_relaxed);
                        s->invalidate(s->render_context, rate_changed, 0);
                    }
            }
            const auto flags = s->notifications.load(std::memory_order_relaxed);
            normalized.notifications = flags;
            normalized.latency_revision = s->latency_revision.load(std::memory_order_acquire);
            if (s->callback_error.load(std::memory_order_relaxed) == 0
                && (flags & (reset_required | rate_changed | resync_required | clock_lost)) == 0) {
                // Windows ASIO callbacks may process inline even when direct is
                // false (SDK p8, 26-27). No extra render queue or mutex is used.
                s->render(s->render_context, s->planes[0].buffers[index],
                          s->planes[1].buffers[index], s->frames,
                          s->channels[0].type, s->channels[1].type, &normalized);
            }
            if (s->callback_error.load(std::memory_order_acquire) != 0
                || (s->notifications.load(std::memory_order_acquire)
                    & (reset_required | rate_changed | resync_required | clock_lost))) {
                clear_planes(s, index);
            }
            if (s->ready) {
                const auto result = s->driver->outputReady();
                if (result != ASE_OK) fault(s, result);
            }
        } catch (...) {
            fault(s, driver_exception);
        }
    }
    s->rendering.clear(std::memory_order_release);
}

void buffer_switch(long index, ASIOBool direct) { process(nullptr, index, direct); }
ASIOTime* time_switch(ASIOTime* time, long index, ASIOBool direct) {
    process(time, index, direct);
    return time;
}
void sample_rate_changed(ASIOSampleRate rate) {
    Entry entry;
    if (auto* s = entry.session) {
        const double observed = std::isfinite(rate) && rate > 0 ? rate : 0;
        uint64_t bits = 0;
        std::memcpy(&bits, &observed, sizeof(bits));
        s->notified_rate_bits.store(bits, std::memory_order_release);
        if (s->rate_notice_count.fetch_add(1, std::memory_order_acq_rel) == UINT64_MAX) {
            s->notifications.fetch_or(clock_lost, std::memory_order_release);
            fault(s, invalid_geometry);
        }
        // SDK sample host documents same-rate notices for external digital input
        // status. They do not imply an output format change or require a restart.
        if (std::isfinite(rate) && rate > 0 && rate == s->configured_rate) {
            s->notifications.fetch_or(same_rate_notice, std::memory_order_relaxed);
            return;
        }
        const auto reason = rate == 0 ? clock_lost : rate_changed;
        s->notifications.fetch_or(reason, std::memory_order_relaxed);
        if (s->invalidate) s->invalidate(s->render_context, reason, 0);
    }
}
long asio_message(long selector, long value, void*, double*) {
    Entry entry;
    auto* s = entry.session;
    if (!s) return 0;
    if (selector == kAsioSelectorSupported) {
        switch (value) {
        case kAsioEngineVersion: case kAsioResetRequest: case kAsioResyncRequest:
        case kAsioLatenciesChanged: case kAsioSupportsTimeInfo: case kAsioOverload: return 1;
        default: return 0;
        }
    }
    switch (selector) {
    case kAsioEngineVersion: return 2;
    case kAsioSupportsTimeInfo: return 1;
    case kAsioResetRequest:
        s->notifications.fetch_or(reset_required);
        if (s->invalidate) s->invalidate(s->render_context, reset_required, 0);
        return 1;
    case kAsioResyncRequest:
        s->notifications.fetch_or(resync_required);
        if (s->invalidate) s->invalidate(s->render_context, resync_required, 0);
        return 1;
    case kAsioLatenciesChanged:
    {
        const auto previous = s->latency_revision.fetch_add(1, std::memory_order_acq_rel);
        if (previous == UINT32_MAX) {
            s->notifications.fetch_or(clock_lost);
            if (s->invalidate) s->invalidate(s->render_context, clock_lost, 0);
            return 1;
        }
        const auto revision = previous + 1;
        if (s->invalidate) s->invalidate(s->render_context, latencies_changed, revision);
        s->notifications.fetch_or(latencies_changed); return 1;
    }
    case kAsioOverload: s->notifications.fetch_or(overload); return 1;
    default: return 0;
    }
}
}

extern "C" int32_t fl_asio_enumerate(void* context, FlAsioEnumerate emit) noexcept {
    if (!emit) return invalid_state;
    HKEY root = nullptr;
    auto error = RegOpenKeyExW(HKEY_LOCAL_MACHINE, L"SOFTWARE\\ASIO", 0, KEY_READ, &root);
    if (error == ERROR_FILE_NOT_FOUND) return 0;
    if (error != ERROR_SUCCESS) return error;
    for (DWORD index = 0;; ++index) {
        wchar_t name[256]{};
        DWORD length = 256;
        error = RegEnumKeyExW(root, index, name, &length, nullptr, nullptr, nullptr, nullptr);
        if (error == ERROR_NO_MORE_ITEMS) { error = ERROR_SUCCESS; break; }
        if (error != ERROR_SUCCESS) break;
        wchar_t clsid[64]{};
        DWORD bytes = sizeof(clsid);
        auto read = RegGetValueW(root, name, L"CLSID", RRF_RT_REG_SZ, nullptr, clsid, &bytes);
        CLSID parsed{};
        if (read == ERROR_SUCCESS && SUCCEEDED(CLSIDFromString(clsid, &parsed))) {
            wchar_t canonical[40]{};
            if (StringFromGUID2(parsed, canonical, 40) != 0)
                emit(context, reinterpret_cast<const uint16_t*>(canonical),
                     reinterpret_cast<const uint16_t*>(name));
        }
    }
    RegCloseKey(root);
    return error;
}

extern "C" int32_t fl_asio_new(FlAsioSession** out) noexcept {
    if (!out) return invalid_state;
    *out = nullptr;
    bool expected = false;
    if (!session_claimed.compare_exchange_strong(expected, true)) return busy_session;
    auto* s = new (std::nothrow) FlAsioSession();
    if (!s) { session_claimed.store(false); return ASE_NoMemory; }
    *out = s;
    const auto result = CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);
    if (FAILED(result)) return result;
    s->com = true;
    s->window = CreateWindowExW(0, L"STATIC", L"Looper ASIO", WS_POPUP,
                               0, 0, 0, 0, nullptr, nullptr, GetModuleHandleW(nullptr), nullptr);
    if (!s->window) return static_cast<int32_t>(HRESULT_FROM_WIN32(GetLastError()));
    return 0;
}

extern "C" int32_t fl_asio_open(FlAsioSession* s, const uint16_t* clsid) noexcept {
    if (!control_owner(s) || s->driver || !s->com || !s->window || !clsid) return invalid_state;
    try {
        CLSID id{};
        auto result = CLSIDFromString(reinterpret_cast<const wchar_t*>(clsid), &id);
        if (FAILED(result)) return result;
        // ASIO drivers expose the registered CLSID as their IASIO IID.
        result = CoCreateInstance(id, nullptr, CLSCTX_INPROC_SERVER, id,
                                  reinterpret_cast<void**>(&s->driver));
        if (FAILED(result)) return result;
        return initialize_selected_driver(s, s->window);
    } catch (...) { return driver_exception; }
}

extern "C" int32_t fl_asio_info(FlAsioSession* s, FlAsioInfo* info) noexcept {
    if (!control_owner(s) || !s->driver || !info) return invalid_state;
    try {
        *info = {};
        long inputs = 0, outputs = 0, min = 0, max = 0, preferred = 0, granularity = 0;
        auto result = s->driver->getChannels(&inputs, &outputs);
        if (result != ASE_OK) return result;
        result = s->driver->getBufferSize(&min, &max, &preferred, &granularity);
        if (result != ASE_OK) return result;
        result = s->driver->getSampleRate(&info->sample_rate);
        if (result != ASE_OK) return result;
        info->output_channels = outputs;
        info->min_frames = min; info->max_frames = max;
        info->preferred_frames = preferred; info->granularity = granularity;
        info->prepared_frames = s->frames;
        info->output_ready = s->ready;
        if (s->buffers) {
            long input_latency = 0, output_latency = 0;
            result = s->driver->getLatencies(&input_latency, &output_latency);
            if (result != ASE_OK) return result;
            if (input_latency < 0 || output_latency < 0) return invalid_geometry;
            info->input_latency_frames = input_latency;
            info->output_latency_frames = output_latency;
        }
        if (!std::isfinite(info->sample_rate) || info->sample_rate <= 0 || outputs < 2
            || min <= 0 || max < min || preferred < min || preferred > max
            || granularity < -1 || !legal_frames(preferred, *info)) return invalid_geometry;
        return 0;
    } catch (...) { return driver_exception; }
}

extern "C" int32_t fl_asio_channel(FlAsioSession* s, int32_t index, FlAsioChannel* out) noexcept {
    if (!control_owner(s) || !s->driver || !out || index < 0) return invalid_state;
    try {
        ASIOChannelInfo info{};
        info.channel = index; info.isInput = ASIOFalse;
        auto result = s->driver->getChannelInfo(&info);
        if (result != ASE_OK) return result;
        out->index = info.channel; out->sample_type = info.type; out->group = info.channelGroup;
        std::memcpy(out->name, info.name, 32); out->name[31] = 0;
        return 0;
    } catch (...) { return driver_exception; }
}

extern "C" int32_t fl_asio_can_rate(FlAsioSession* s, double rate) noexcept {
    if (!control_owner(s) || !s->driver || !std::isfinite(rate) || rate <= 0) return invalid_state;
    try { return s->driver->canSampleRate(rate); } catch (...) { return driver_exception; }
}
extern "C" int32_t fl_asio_set_rate(FlAsioSession* s, double rate) noexcept {
    if (!control_owner(s) || !s->driver || s->buffers || !std::isfinite(rate) || rate <= 0)
        return invalid_state;
    if (!(s->rate.flags & 1) || s->rate.flags & (2 | 4)) return invalid_state;
    s->rate.flags |= 2;
    s->rate.target_rate = rate;
    s->rate.query_status = invalid_state;
    // Record the attempted mutation before calling foreign code. A failed
    // setter can still have changed the driver-global rate.
    try { s->rate.set_status = s->driver->setSampleRate(rate); }
    catch (...) { s->rate.set_status = driver_exception; }
    try { s->rate.query_status = s->driver->getSampleRate(&s->rate.observed_rate); }
    catch (...) { s->rate.query_status = driver_exception; }
    if (s->rate.set_status != ASE_OK) return s->rate.set_status;
    if (s->rate.query_status != ASE_OK) return s->rate.query_status;
    return s->rate.observed_rate == rate ? ASE_OK : invalid_geometry;
}

extern "C" int32_t fl_asio_commit_rate(FlAsioSession* s) noexcept {
    if (!control_owner(s) || !s->driver || !s->start_attempted
        || s->callback_error.load(std::memory_order_acquire) != 0
        || (s->notifications.load(std::memory_order_acquire)
            & (reset_required | rate_changed | resync_required | clock_lost))) return invalid_state;
    try {
        double current_rate = 0;
        const auto result = s->driver->getSampleRate(&current_rate);
        if (result != ASE_OK) return result;
        if (current_rate != s->configured_rate) return invalid_geometry;
        if (s->callback_error.load(std::memory_order_acquire) != 0
            || (s->notifications.load(std::memory_order_acquire)
                & (reset_required | rate_changed | resync_required | clock_lost))) return invalid_state;
        s->rate.flags |= 4; s->rate.disposition = 1;
        return 0;
    } catch (...) { return driver_exception; }
}

extern "C" int32_t fl_asio_rate_transaction(FlAsioSession* s, FlAsioRateTransaction* out) noexcept {
    if (!s || !out || s->thread != GetCurrentThreadId()) return invalid_state;
    *out = s->rate;
    return 0;
}

extern "C" int32_t fl_asio_abort_candidate_rate(FlAsioSession* s) noexcept {
    if (!control_owner(s) || !s->driver) return invalid_state;
    // Rust admits this only for the stream installed by the STILL ACTIVE odd
    // output transaction. Ordinary close of a completed configuration never
    // reaches this API, including its close during the next output change.
    s->rate.flags &= ~uint32_t{4};
    s->rate.disposition = 0;
    return 0;
}

extern "C" int32_t fl_asio_prepare(FlAsioSession* s, int32_t frames, int32_t left,
                                    int32_t right, void* context, FlAsioRender render,
                                    FlAsioInvalidate invalidate) noexcept {
    if (!control_owner(s) || !s->driver || s->buffers || !context || !render || !invalidate || left == right)
        return invalid_state;
    try {
        FlAsioInfo info{};
        auto result = fl_asio_info(s, &info);
        if (result != 0) return result;
        if (!legal_frames(frames, info) || left < 0 || right < 0
            || left >= info.output_channels || right >= info.output_channels) return invalid_geometry;
        const int32_t selected[2]{left, right};
        for (int i = 0; i < 2; ++i) {
            s->channels[i].channel = selected[i]; s->channels[i].isInput = ASIOFalse;
            result = s->driver->getChannelInfo(&s->channels[i]);
            if (result != ASE_OK) return result;
            if (s->channels[i].channel != selected[i] || s->channels[i].isInput != ASIOFalse
                || !sample_bytes(s->channels[i].type)) return invalid_geometry;
            s->planes[i].isInput = ASIOFalse; s->planes[i].channelNum = selected[i];
        }
        s->frames = frames; s->configured_rate = info.sample_rate;
        s->render_context = context; s->render = render;
        s->invalidate = invalidate;
        s->callbacks = {buffer_switch, sample_rate_changed, asio_message, time_switch};
        // Both the Rust context and bridge are complete before any driver callback.
        current.store(s, std::memory_order_seq_cst);
        s->buffers = true; // Retain custody even if a broken create partially fails.
        result = s->driver->createBuffers(s->planes, 2, frames, &s->callbacks);
        if (result != ASE_OK) return result;
        for (const auto& plane : s->planes)
            if (!plane.buffers[0] || !plane.buffers[1]) return invalid_geometry;
        uintptr_t starts[4]{};
        uintptr_t ends[4]{};
        for (int i = 0; i < 4; ++i) {
            const auto channel = i / 2;
            const auto bytes = static_cast<size_t>(frames) * sample_bytes(s->channels[channel].type);
            starts[i] = reinterpret_cast<uintptr_t>(s->planes[channel].buffers[i % 2]);
            if (starts[i] > UINTPTR_MAX - bytes) return invalid_geometry;
            ends[i] = starts[i] + bytes;
            for (int previous = 0; previous < i; ++previous)
                if (starts[i] < ends[previous] && starts[previous] < ends[i]) return invalid_geometry;
        }
        result = s->driver->outputReady();
        if (result != ASE_OK && result != ASE_NotPresent) return result;
        s->ready = result == ASE_OK;
        clear_planes(s, 0); clear_planes(s, 1);
        if (s->ready) {
            result = s->driver->outputReady();
            if (result != ASE_OK) return result;
        }
        s->prepared.store(true, std::memory_order_release);
        return 0;
    } catch (...) { return driver_exception; }
}

extern "C" int32_t fl_asio_start(FlAsioSession* s) noexcept {
    if (!control_owner(s) || !s->prepared.load(std::memory_order_acquire)
        || s->start_attempted || s->detached
        || s->callback_error.load(std::memory_order_acquire) != 0
        || (s->notifications.load(std::memory_order_acquire)
            & (reset_required | rate_changed | resync_required | clock_lost))) return invalid_state;
    s->start_attempted = true; // Start may prime synchronously and fail after callbacks.
    try { return s->driver->start(); } catch (...) { return driver_exception; }
}

extern "C" int32_t fl_asio_close(FlAsioSession* s) noexcept {
    if (!s || s->thread != GetCurrentThreadId()) return invalid_state;
    if (s->closed) return 0;
    try {
        if (s->start_attempted) {
            const auto result = s->driver->stop();
            if (result != ASE_OK) return result;
            s->start_attempted = false;
        }
        // Successful Stop guarantees no later buffer switch. Notifications may
        // still arrive during Dispose; they use only the independent entry guard.
        current.store(nullptr, std::memory_order_seq_cst);
        s->prepared.store(false, std::memory_order_release);
        s->detached = true;
        while (callback_entries.load(std::memory_order_seq_cst) != 0) Sleep(1);
        if (s->buffers) {
            const auto result = s->driver->disposeBuffers();
            if (result != ASE_OK) return result;
            s->buffers = false;
        }
        abort_rate_transaction(s);
        if (s->driver) { s->driver->Release(); s->driver = nullptr; }
        if (s->window) { DestroyWindow(s->window); s->window = nullptr; }
        if (s->com) { CoUninitialize(); s->com = false; }
        s->render = nullptr; s->render_context = nullptr;
        s->invalidate = nullptr;
        s->closed = true;
        return 0;
    } catch (...) { return driver_exception; }
}
extern "C" int32_t fl_asio_delete(FlAsioSession* s) noexcept {
    if (!s || !s->closed || s->thread != GetCurrentThreadId()) return invalid_state;
    delete s;
    session_claimed.store(false);
    return 0;
}
extern "C" void fl_asio_status(FlAsioSession* s, FlAsioStatus* out) noexcept {
    if (!s || !out) return;
    out->notifications = s->notifications.load(std::memory_order_acquire);
    out->callback_error = s->callback_error.load(std::memory_order_acquire);
    out->in_flight = callback_entries.load(std::memory_order_seq_cst);
    out->latency_revision = s->latency_revision.load(std::memory_order_acquire);
    const auto bits = s->notified_rate_bits.load(std::memory_order_acquire);
    std::memcpy(&out->last_notified_rate, &bits, sizeof(bits));
    out->rate_notice_count = s->rate_notice_count.load(std::memory_order_acquire);
}
extern "C" void fl_asio_pump_messages() noexcept {
    MSG message{};
    for (unsigned i = 0; i < 64 && PeekMessageW(&message, nullptr, 0, 0, PM_REMOVE); ++i) {
        TranslateMessage(&message); DispatchMessageW(&message);
    }
}

#ifdef FLITZIS_ASIO_CONTRACT_TESTS
#include "asio_output_contract.h"
#endif
