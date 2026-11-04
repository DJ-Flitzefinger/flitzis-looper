// Private hardware-free contract fixture. Included only by asio-contract-tests.
// into the SAME bridge translation unit; no Registry, COM factory or real driver.
#include <thread>
#include <chrono>
#include <algorithm>

// One private per-Engine device state outlives disposable sessions. This models
// driver-global rate custody without any Registry, COM or physical device access.
struct FlAsioTestState {
    std::atomic<uint64_t> rate_bits{0};
    std::atomic<uint64_t> created{0}, released{0}, active{0};
    std::atomic<uint64_t> fail_start_session{0};
    std::atomic<uint64_t> fail_tick_session{0};
    FlAsioTestState() {
        const double value = 48000;
        uint64_t bits = 0; std::memcpy(&bits, &value, sizeof(bits)); rate_bits.store(bits);
    }
    double rate() const {
        const auto bits = rate_bits.load(std::memory_order_acquire);
        double value = 0; std::memcpy(&value, &bits, sizeof(value)); return value;
    }
    void set_rate(double value) {
        uint64_t bits = 0; std::memcpy(&bits, &value, sizeof(bits));
        rate_bits.store(bits, std::memory_order_release);
    }
};

struct FlAsioTestDeviceInfo {
    double rate;
    uint64_t created, released, active;
};
struct FlAsioTestTick {
    uint64_t callback_ticks, qpc_frequency, output_ready_calls;
    uint32_t frames, guards_intact;
    double left_peak, right_peak;
};
static_assert(sizeof(FlAsioTestDeviceInfo) == 32 && sizeof(FlAsioTestTick) == 48);

namespace {
struct ContractFailure { int line; };
#define FL_CHECK(condition) do { if (!(condition)) throw ContractFailure{__LINE__}; } while (false)

struct FakeAsio;
struct FakeRender {
    FakeAsio* driver = nullptr;
    std::atomic<unsigned> calls{0}, invalidations{0};
    std::atomic<bool> entered{false}, exited{false};
    bool reenter = false, block = false, notify_during_render = false;
    uint32_t last_flags = 0;
};

struct FakeAsio final : IASIO {
    FlAsioTestState* state = nullptr;
    uint64_t session_number = 0;
    uint64_t next_position = 0;
    uint32_t next_index = 0;
    bool heap_owned = false, started = false;
    ULONG references = 1;
    double rate = 48000;
    long minimum = 96, maximum = 96, preferred = 96, granularity = 0;
    ASIOError create_result = ASE_OK, start_result = ASE_OK, stop_result = ASE_OK, dispose_result = ASE_OK;
    ASIOError set_result = ASE_OK, ready_result = ASE_OK;
    bool prime = false, early_callback = false, throw_set = false, throw_restore = false;
    bool notify_on_rate_query = false, notify_on_latency_query = false;
    std::atomic<bool> stop_signalled{false};
    unsigned creates = 0, starts = 0, stops = 0, disposes = 0, releases = 0;
    unsigned setters = 0, rate_queries = 0, ready_calls = 0, position_queries = 0;
    long selected[2]{-1, -1};
    ASIOCallbacks callbacks{};
    // Distinct planes with two eight-byte sentinels each; no heap in callbacks.
    unsigned char storage[2][2][512 * 8 + 16]{};
    long frames = 0;
    FakeRender* context = nullptr;

    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID, void** out) override {
        if (out) *out = nullptr;
        return E_NOINTERFACE;
    }
    ULONG STDMETHODCALLTYPE AddRef() override { return ++references; }
    ULONG STDMETHODCALLTYPE Release() override {
        ++releases; const auto remaining = --references;
        if (!remaining && heap_owned) {
            state->released.fetch_add(1); state->active.fetch_sub(1); delete this;
        }
        return remaining;
    }
    ASIOBool init(void*) override { return ASIOTrue; }
    void getDriverName(char* name) override { std::strcpy(name, "Private contract fixture"); }
    long getDriverVersion() override { return 1; }
    void getErrorMessage(char* message) override { std::strcpy(message, "Injected contract failure"); }
    ASIOError start() override {
        ++starts;
        if (state && state->fail_start_session.load() == session_number) return ASE_HWMalfunction;
        if (prime) { tick(0, 0, 1000000); tick(1, frames, 1000000); }
        started = start_result == ASE_OK;
        return start_result;
    }
    ASIOError stop() override { ++stops; stop_signalled.store(true); if (stop_result == ASE_OK) started = false; return stop_result; }
    ASIOError getChannels(long* input, long* output) override { *input = 0; *output = 8; return ASE_OK; }
    ASIOError getLatencies(long* input, long* output) override {
        *input = 7; *output = frames + 19;
        if (notify_on_latency_query) callbacks.asioMessage(kAsioLatenciesChanged, 0, nullptr, nullptr);
        return ASE_OK;
    }
    ASIOError getBufferSize(long* min, long* max, long* pref, long* gran) override {
        *min = minimum; *max = maximum; *pref = preferred; *gran = granularity; return ASE_OK;
    }
    ASIOError canSampleRate(ASIOSampleRate value) override { return value == 48000 || value == 96000 ? ASE_OK : ASE_NoClock; }
    ASIOError getSampleRate(ASIOSampleRate* value) override {
        ++rate_queries; *value = state ? state->rate() : rate;
        if (notify_on_rate_query && callbacks.asioMessage) callbacks.asioMessage(kAsioResetRequest, 0, nullptr, nullptr);
        return ASE_OK;
    }
    ASIOError setSampleRate(ASIOSampleRate value) override {
        ++setters; rate = value;
        if (state) state->set_rate(value);
        if (throw_set || (throw_restore && value == 48000)) throw 1;
        return set_result;
    }
    ASIOError getClockSources(ASIOClockSource*, long* count) override { *count = 0; return ASE_OK; }
    ASIOError setClockSource(long) override { return ASE_NotPresent; }
    ASIOError getSamplePosition(ASIOSamples* position, ASIOTimeStamp* time) override {
        ++position_queries; *position = {0, 960}; *time = {0, 10000000}; return ASE_OK;
    }
    ASIOError getChannelInfo(ASIOChannelInfo* channel) override {
        channel->channelGroup = channel->channel / 2;
        channel->type = channel->channel == 2 ? ASIOSTInt16LSB : ASIOSTFloat64LSB;
        std::strcpy(channel->name, "Physical fixture output"); return ASE_OK;
    }
    ASIOError createBuffers(ASIOBufferInfo* planes, long count, long size, ASIOCallbacks* cb) override {
        ++creates; frames = size; callbacks = *cb;
        FL_CHECK(count == 2 && size > 0 && size <= 512);
        for (int channel = 0; channel < 2; ++channel) {
            FL_CHECK(planes[channel].isInput == ASIOFalse);
            selected[channel] = planes[channel].channelNum;
            for (int index = 0; index < 2; ++index) {
                std::memset(storage[channel][index], 0xa5, sizeof(storage[channel][index]));
                planes[channel].buffers[index] = storage[channel][index] + 8;
            }
        }
        if (early_callback) tick(0);
        return create_result;
    }
    ASIOError disposeBuffers() override {
        ++disposes;
        if (context && context->block) FL_CHECK(context->exited.load());
        return dispose_result;
    }
    ASIOError controlPanel() override { return ASE_NotPresent; }
    ASIOError future(long, void*) override { return ASE_NotPresent; }
    ASIOError outputReady() override { ++ready_calls; return ready_result; }

    void tick(long index, uint64_t position = 192, uint64_t ns = 2000000, unsigned extra = 0) {
        ASIOTime time{};
        time.timeInfo.samplePosition = {static_cast<unsigned long>(position >> 32), static_cast<unsigned long>(position)};
        time.timeInfo.systemTime = {static_cast<unsigned long>(ns >> 32), static_cast<unsigned long>(ns)};
        time.timeInfo.sampleRate = state ? state->rate() : rate;
        time.timeInfo.flags = kSystemTimeValid | kSamplePositionValid | kSampleRateValid | extra;
        callbacks.bufferSwitchTimeInfo(&time, index, ASIOTrue);
    }
    bool guards_intact() const {
        for (int channel = 0; channel < 2; ++channel) {
            const auto bytes = frames * (selected[channel] == 2 ? 2 : 8);
            for (int index = 0; index < 2; ++index)
                for (int byte = 0; byte < 8; ++byte)
                    if (storage[channel][index][byte] != 0xa5
                        || storage[channel][index][8 + bytes + byte] != 0xa5) return false;
        }
        return true;
    }
};

void contract_invalidate(void* opaque, uint32_t, uint32_t) {
    static_cast<FakeRender*>(opaque)->invalidations.fetch_add(1);
}
void contract_render(void* opaque, void* left, void* right, int32_t frames,
                     int32_t left_type, int32_t right_type, const FlAsioTime* time) {
    auto& context = *static_cast<FakeRender*>(opaque);
    context.calls.fetch_add(1); context.last_flags = time->flags;
    context.entered.store(true);
    if (context.block) {
        while (!context.driver->stop_signalled.load()) std::this_thread::yield();
        std::this_thread::sleep_for(std::chrono::milliseconds(2));
    }
    if (context.reenter) { context.reenter = false; context.driver->tick(0); }
    if (context.notify_during_render) context.driver->callbacks.asioMessage(kAsioResetRequest, 0, nullptr, nullptr);
    std::memset(left, 0x44, static_cast<size_t>(frames) * sample_bytes(left_type));
    std::memset(right, 0x11, static_cast<size_t>(frames) * sample_bytes(right_type));
    context.exited.store(true);
}

struct ContractSession {
    FakeAsio driver;
    FakeRender render;
    FlAsioSession* session = nullptr;
    std::thread callback;
    ContractSession() {
        bool expected = false;
        FL_CHECK(session_claimed.compare_exchange_strong(expected, true));
        session = new FlAsioSession();
        session->driver = &driver;
        render.driver = &driver; driver.context = &render;
        FL_CHECK(initialize_selected_driver(session, nullptr) == ASE_OK);
    }
    ~ContractSession() {
        driver.stop_result = driver.dispose_result = ASE_OK;
        driver.notify_on_rate_query = false;
        if (session) {
            const auto result = fl_asio_close(session);
            if (callback.joinable()) callback.join();
            if (result == 0) fl_asio_delete(session);
        }
    }
    void prepare() {
        FL_CHECK(fl_asio_prepare(session, 96, 2, 5, &render, contract_render, contract_invalidate) == 0);
    }
    void start() { FL_CHECK(fl_asio_start(session) == 0); }
    FlAsioRateTransaction close() {
        FL_CHECK(fl_asio_close(session) == 0);
        if (callback.joinable()) callback.join();
        FlAsioRateTransaction receipt{};
        FL_CHECK(fl_asio_rate_transaction(session, &receipt) == 0);
        FL_CHECK(driver.releases == 1 && callback_entries.load() == 0 && current.load() == nullptr);
        FL_CHECK(fl_asio_delete(session) == 0); session = nullptr;
        return receipt;
    }
};
}

extern "C" int32_t fl_asio_test_state_new(FlAsioTestState** out) noexcept {
    if (!out) return invalid_state;
    *out = new (std::nothrow) FlAsioTestState();
    return *out ? 0 : ASE_NoMemory;
}
extern "C" int32_t fl_asio_test_state_delete(FlAsioTestState* state) noexcept {
    if (!state || state->active.load() != 0) return invalid_state;
    delete state; return 0;
}
extern "C" int32_t fl_asio_test_state_info(FlAsioTestState* state, FlAsioTestDeviceInfo* out) noexcept {
    if (!state || !out) return invalid_state;
    *out = {state->rate(), state->created.load(), state->released.load(), state->active.load()};
    return 0;
}
extern "C" int32_t fl_asio_test_fail_start(FlAsioTestState* state, uint64_t session_number) noexcept {
    if (!state) return invalid_state;
    state->fail_start_session.store(session_number); return 0;
}
extern "C" int32_t fl_asio_test_fail_tick(FlAsioTestState* state, uint64_t session_number) noexcept {
    if (!state) return invalid_state;
    state->fail_tick_session.store(session_number); return 0;
}
extern "C" int32_t fl_asio_test_new(FlAsioTestState* state, FlAsioSession** out) noexcept {
    if (!state || !out) return invalid_state;
    *out = nullptr;
    bool expected = false;
    if (!session_claimed.compare_exchange_strong(expected, true)) return busy_session;
    auto* session = new (std::nothrow) FlAsioSession();
    if (!session) { session_claimed.store(false); return ASE_NoMemory; }
    *out = session; // The original Owner adopts partial acquisition before errors.
    session->test_state = state;
    auto* driver = new (std::nothrow) FakeAsio();
    if (!driver) return ASE_NoMemory;
    session->driver = driver;
    driver->state = state; driver->heap_owned = true;
    driver->session_number = state->created.fetch_add(1) + 1;
    state->active.fetch_add(1);
    driver->minimum = 96; driver->maximum = 512; driver->preferred = 96; driver->granularity = 1;
    try { return initialize_selected_driver(session, nullptr); }
    catch (...) { return driver_exception; }
}
extern "C" int32_t fl_asio_test_tick(FlAsioSession* session, FlAsioTestState* state,
                                      uint64_t system_ns, FlAsioTestTick* out) noexcept {
    if (!control_owner(session) || !state || session->test_state != state || !out
        || !session->driver || session->detached || !session->prepared.load()
        || session->callback_error.load() != 0) return invalid_state;
    auto* driver = static_cast<FakeAsio*>(session->driver);
    if (!driver->heap_owned || driver->state != state || !driver->started) return invalid_state;
    try {
        // A genuine shared callback-fault path after successful Open allows the
        // App test to exercise rollback AFTER its one-time grid conversion.
        if (state->fail_tick_session.load() == driver->session_number) fault(session, ASE_HWMalfunction);
        ASIOTime time{};
        const auto position = driver->next_position;
        time.timeInfo.samplePosition = {static_cast<unsigned long>(position >> 32), static_cast<unsigned long>(position)};
        time.timeInfo.systemTime = {static_cast<unsigned long>(system_ns >> 32), static_cast<unsigned long>(system_ns)};
        time.timeInfo.sampleRate = state->rate();
        time.timeInfo.flags = kSystemTimeValid | kSamplePositionValid | kSampleRateValid;
        LARGE_INTEGER frequency{}, begin{}, end{};
        if (!QueryPerformanceFrequency(&frequency) || frequency.QuadPart <= 0
            || !QueryPerformanceCounter(&begin)) return invalid_state;
        const auto index = driver->next_index;
        driver->callbacks.bufferSwitchTimeInfo(&time, index, ASIOTrue);
        if (!QueryPerformanceCounter(&end) || end.QuadPart < begin.QuadPart) return invalid_state;
        *out = {static_cast<uint64_t>(end.QuadPart - begin.QuadPart), static_cast<uint64_t>(frequency.QuadPart),
                driver->ready_calls, static_cast<uint32_t>(driver->frames), driver->guards_intact() ? 1u : 0u, 0, 0};
        double* peaks[2]{&out->left_peak, &out->right_peak};
        for (int channel = 0; channel < 2; ++channel) {
            const auto* bytes = static_cast<const unsigned char*>(session->planes[channel].buffers[index]);
            for (long frame = 0; frame < driver->frames; ++frame) {
                double value = 0;
                if (session->channels[channel].type == ASIOSTInt16LSB) {
                    int16_t integer = 0; std::memcpy(&integer, bytes + frame * 2, 2);
                    value = static_cast<double>(integer) / 32768.0;
                } else { std::memcpy(&value, bytes + frame * 8, 8); }
                if (!std::isfinite(value)) return invalid_geometry;
                *peaks[channel] = (std::max)(*peaks[channel], std::abs(value));
            }
        }
        driver->next_position += static_cast<uint64_t>(driver->frames);
        driver->next_index ^= 1;
        return out->guards_intact ? 0 : invalid_geometry;
    } catch (...) { return driver_exception; }
}

extern "C" int32_t fl_asio_contract_tests(uint32_t* completed, uint32_t* failed_line) noexcept {
    if (!completed || !failed_line) return invalid_state;
    *completed = 0; *failed_line = 0;
    try {
        { // SDK granularity, including a non-power-of-two minimum.
            FlAsioInfo info{}; info.min_frames = 48; info.max_frames = 384; info.preferred_frames = 96;
            info.granularity = -1;
            FL_CHECK(legal_frames(48, info) && legal_frames(96, info) && legal_frames(384, info) && !legal_frames(72, info));
            info.granularity = 24; FL_CHECK(legal_frames(72, info) && !legal_frames(60, info));
            info.granularity = 0; FL_CHECK(legal_frames(96, info) && !legal_frames(192, info));
            ++*completed;
        }
        {
            ContractSession test; test.driver.prime = true;
            FL_CHECK(fl_asio_set_rate(test.session, 96000) == 0); test.prepare(); test.start();
            FL_CHECK(test.driver.selected[0] == 2 && test.driver.selected[1] == 5 && test.render.calls == 2);
            FL_CHECK(test.driver.guards_intact() && test.driver.ready_calls == 4);
            FL_CHECK(test.close().disposition == 2 && test.driver.rate == 48000); ++*completed;
        }
        {
            ContractSession test; test.driver.ready_result = ASE_NotPresent; test.prepare(); test.start(); test.driver.tick(0);
            FL_CHECK(test.driver.ready_calls == 1 && test.render.calls == 1 && test.driver.guards_intact());
            test.close(); ++*completed;
        }
        {
            ContractSession test; test.driver.prime = true; test.driver.start_result = ASE_HWMalfunction; test.prepare();
            FL_CHECK(fl_asio_start(test.session) == ASE_HWMalfunction && test.render.calls == 2 && test.driver.releases == 0);
            test.close(); FL_CHECK(test.driver.stops == 1 && test.driver.disposes == 1); ++*completed;
        }
        {
            ContractSession test; test.driver.create_result = ASE_HWMalfunction;
            FL_CHECK(fl_asio_prepare(test.session, 96, 2, 5, &test.render, contract_render, contract_invalidate) == ASE_HWMalfunction);
            test.close(); FL_CHECK(test.driver.disposes == 1 && test.driver.stops == 0); ++*completed;
        }
        {
            ContractSession test; FL_CHECK(fl_asio_set_rate(test.session, 96000) == 0); test.prepare(); test.start();
            test.driver.stop_result = ASE_HWMalfunction;
            FL_CHECK(fl_asio_close(test.session) == ASE_HWMalfunction && test.driver.disposes == 0 && test.driver.setters == 1);
            test.driver.stop_result = ASE_OK; test.driver.dispose_result = ASE_HWMalfunction;
            FL_CHECK(fl_asio_close(test.session) == ASE_HWMalfunction && test.driver.releases == 0 && test.driver.setters == 1);
            test.driver.dispose_result = ASE_OK;
            FL_CHECK(test.close().disposition == 2 && test.driver.setters == 2 && test.driver.disposes == 2); ++*completed;
        }
        {
            ContractSession test; test.driver.throw_set = true;
            FL_CHECK(fl_asio_set_rate(test.session, 96000) == driver_exception && test.driver.rate == 96000);
            test.driver.throw_set = false; auto receipt = test.close();
            FL_CHECK(receipt.set_status == driver_exception && receipt.observed_rate == 96000 && receipt.disposition == 2); ++*completed;
        }
        {
            ContractSession test; FL_CHECK(fl_asio_set_rate(test.session, 96000) == 0); test.driver.rate = 88200;
            FL_CHECK(test.close().disposition == 3 && test.driver.rate == 88200 && test.driver.setters == 1); ++*completed;
        }
        {
            ContractSession test; FL_CHECK(fl_asio_set_rate(test.session, 96000) == 0); test.driver.throw_restore = true;
            auto receipt = test.close();
            FL_CHECK(receipt.disposition == 5 && receipt.restore_set_status == driver_exception
                && receipt.restore_query_status == 0 && receipt.restore_after_rate == 48000); ++*completed;
        }
        {
            ContractSession test; FL_CHECK(fl_asio_set_rate(test.session, 96000) == 0); test.prepare(); test.start();
            FL_CHECK(fl_asio_commit_rate(test.session) == 0);
            FL_CHECK(test.close().disposition == 1 && test.driver.rate == 96000 && test.driver.setters == 1); ++*completed;
        }
        {
            ContractSession test; FL_CHECK(fl_asio_set_rate(test.session, 96000) == 0); test.prepare(); test.start();
            FL_CHECK(fl_asio_commit_rate(test.session) == 0 && fl_asio_abort_candidate_rate(test.session) == 0);
            FL_CHECK(test.close().disposition == 2 && test.driver.rate == 48000); ++*completed;
        }
        {
            ContractSession test; test.prepare(); test.start(); test.driver.notify_on_rate_query = true;
            FL_CHECK(fl_asio_commit_rate(test.session) == invalid_state);
            FlAsioRateTransaction receipt{}; fl_asio_rate_transaction(test.session, &receipt);
            FL_CHECK((receipt.flags & 4) == 0 && test.render.invalidations > 0); test.close(); ++*completed;
        }
        {
            ContractSession test; test.prepare(); test.start(); test.driver.tick(0, 192, 2000000, kSampleRateChanged);
            FL_CHECK(test.render.calls == 1 && test.render.invalidations == 0);
            test.driver.callbacks.sampleRateDidChange(48000); FL_CHECK(test.render.invalidations == 0);
            FlAsioStatus same{}; fl_asio_status(test.session, &same);
            FL_CHECK(same.last_notified_rate == 48000 && same.rate_notice_count == 1);
            test.driver.callbacks.sampleRateDidChange(44100); test.driver.tick(1);
            FlAsioStatus changed{}; fl_asio_status(test.session, &changed);
            FL_CHECK(changed.last_notified_rate == 44100 && changed.rate_notice_count == 2);
            FL_CHECK(test.render.calls == 1 && test.render.invalidations > 0); test.close(); ++*completed;
        }
        {
            ContractSession test; test.prepare(); test.start(); test.driver.notify_on_latency_query = true;
            FlAsioInfo info{}; FL_CHECK(fl_asio_info(test.session, &info) == 0);
            FlAsioStatus status{}; fl_asio_status(test.session, &status);
            FL_CHECK(status.latency_revision == 1 && (status.notifications & latencies_changed) && test.render.invalidations > 0);
            test.close(); ++*completed;
        }
        {
            ContractSession test; test.prepare(); test.start(); test.render.reenter = true; test.driver.tick(0);
            FlAsioStatus status{}; fl_asio_status(test.session, &status);
            FL_CHECK(test.render.calls == 1 && status.callback_error == callback_reentry && test.driver.guards_intact());
            FL_CHECK(test.driver.storage[0][0][8] == 0 && test.driver.storage[1][0][8] == 0); test.close(); ++*completed;
        }
        {
            ContractSession test; test.driver.early_callback = true; test.prepare();
            FL_CHECK(test.render.calls == 0 && test.render.invalidations > 0 && fl_asio_start(test.session) == invalid_state);
            FL_CHECK(test.driver.starts == 0); test.close(); ++*completed;
        }
        {
            ContractSession test; test.prepare(); test.start(); test.driver.tick(3);
            FlAsioStatus status{}; fl_asio_status(test.session, &status);
            FL_CHECK(test.render.calls == 0 && status.callback_error == invalid_callback_index && test.driver.guards_intact());
            test.close(); ++*completed;
        }
        {
            ContractSession test; test.prepare(); test.start(); test.driver.callbacks.bufferSwitch(0, ASIOFalse);
            FL_CHECK(test.driver.position_queries == 1 && test.render.calls == 1 && (test.render.last_flags & kSamplePositionValid));
            test.close(); ++*completed;
        }
        {
            ContractSession test; test.prepare(); test.start(); test.render.block = true;
            test.callback = std::thread([&] { test.driver.tick(0); });
            const auto deadline = GetTickCount64() + 2000;
            while (!test.render.entered.load() && GetTickCount64() < deadline) Sleep(1);
            FL_CHECK(test.render.entered.load());
            test.close(); FL_CHECK(test.render.exited && test.driver.disposes == 1); ++*completed;
        }
        return 0;
    } catch (const ContractFailure& error) { *failed_line = error.line; return invalid_state; }
    catch (...) { return driver_exception; }
}
#undef FL_CHECK
