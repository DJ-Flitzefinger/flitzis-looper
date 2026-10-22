#include "signalsmith_probe.h"
#include <algorithm>
#include <array>
#include <cmath>
#include <cstddef>
#include <exception>
#include <limits>
#include <memory>
#include <new>
#include <random>
#include <vector>

// The pinned minimal source closure uses only the portable built-in FFT.
#if defined(SIGNALSMITH_USE_PFFFT) || defined(SIGNALSMITH_USE_PFFFT_DOUBLE) || \
    defined(SIGNALSMITH_USE_ACCELERATE) || defined(SIGNALSMITH_USE_IPP) || \
    defined(SIGNALSMITH_USE_CMSISDSP) || defined(SIGNALSMITH_USE_XSIMD)
#error This fixed prerequisite does not permit a different FFT implementation
#endif
#include "signalsmith-stretch.h"

struct FlitzisSignalsmithProbe {
    signalsmith::stretch::SignalsmithStretch<float, std::mt19937> stretch{0};
    bool terminal = false;
    uint32_t processed = 0;
};

namespace {
constexpr uint32_t max_input = 2000000;
constexpr uint32_t max_output = 1000000;
constexpr uint32_t max_process = 512;

bool disjoint(const float *a, uint32_t na, const float *b, uint32_t nb) noexcept {
    const auto start_a = reinterpret_cast<uintptr_t>(a);
    const auto start_b = reinterpret_cast<uintptr_t>(b);
    const auto bytes_a = static_cast<uintptr_t>(na) * sizeof(float);
    const auto bytes_b = static_cast<uintptr_t>(nb) * sizeof(float);
    if (start_a > std::numeric_limits<uintptr_t>::max() - bytes_a ||
        start_b > std::numeric_limits<uintptr_t>::max() - bytes_b) return false;
    return start_a + bytes_a <= start_b || start_b + bytes_b <= start_a;
}

bool valid_output(float *left, float *right, uint32_t frames) noexcept {
    return left && right && frames > 0 && frames <= max_output &&
        disjoint(left, frames, right, frames);
}

bool finite_output(const float *left, const float *right, uint32_t frames) noexcept {
    for (uint32_t i = 0; i < frames; ++i) {
        if (!std::isfinite(left[i]) || !std::isfinite(right[i])) return false;
    }
    return true;
}
}

extern "C" int32_t flitzis_signalsmith_probe_create(
    double pitch, FlitzisSignalsmithInfo *info, FlitzisSignalsmithProbe **owner
) noexcept {
    if (!owner || !info) return 1;
    *owner = nullptr;
    if (pitch != 1.0 && pitch != 2.0) return 1;
    try {
        auto candidate = std::unique_ptr<FlitzisSignalsmithProbe>(new FlitzisSignalsmithProbe);
        candidate->stretch.presetDefault(2, 44100.0f, false);
        candidate->stretch.setTransposeFactor(static_cast<float>(pitch), 0.0f);
        const auto &s = candidate->stretch;
        *info = {
            static_cast<uint32_t>(s.version[0]), static_cast<uint32_t>(s.version[1]),
            static_cast<uint32_t>(s.version[2]), 2, 44100,
            static_cast<uint32_t>(s.blockSamples()), static_cast<uint32_t>(s.intervalSamples()),
            static_cast<uint32_t>(s.inputLatency()), static_cast<uint32_t>(s.outputLatency()),
            static_cast<uint32_t>(s.splitComputation()), 0
        };
        *owner = candidate.release();
        return 0;
    } catch (const std::bad_alloc &) {
        return 2;
    } catch (const std::exception &) {
        return 3;
    } catch (...) {
        return 3; // Unknown C++ exceptions must never cross the C ABI.
    }
}

extern "C" int32_t flitzis_signalsmith_probe_process(
    FlitzisSignalsmithProbe *owner,
    const float *left, const float *right,
    float *out_left, float *out_right, uint32_t frames
) noexcept {
    if (!owner) return 1;
    if (owner->terminal) return 4;
    if (!left || !right || frames > max_process || !valid_output(out_left, out_right, frames) ||
        !disjoint(left, frames, out_left, frames) || !disjoint(left, frames, out_right, frames) ||
        !disjoint(right, frames, out_left, frames) || !disjoint(right, frames, out_right, frames) ||
        !finite_output(left, right, frames) || owner->processed > max_input - frames) return 1;
    owner->terminal = true;
    try {
        const float *input[2] = {left, right};
        float *output[2] = {out_left, out_right};
        owner->stretch.process(input, static_cast<int>(frames), output, static_cast<int>(frames));
        if (!finite_output(out_left, out_right, frames)) return 3;
        owner->processed += frames;
        owner->terminal = false;
        return 0;
    } catch (const std::bad_alloc &) {
        return 2;
    } catch (const std::exception &) {
        return 3;
    } catch (...) {
        return 3;
    }
}

extern "C" int32_t flitzis_signalsmith_probe_flush(
    FlitzisSignalsmithProbe *owner, float *out_left, float *out_right, uint32_t frames
) noexcept {
    if (!owner) return 1;
    if (owner->terminal) return 4;
    const auto expected = owner->stretch.outputLatency() + 2 * owner->stretch.blockSamples();
    if (frames != static_cast<uint32_t>(expected) || !valid_output(out_left, out_right, frames)) return 1;
    const auto generated_input = frames - static_cast<uint32_t>(owner->stretch.intervalSamples());
    if (generated_input > max_input || owner->processed > max_input - generated_input) return 1;
    owner->terminal = true; // Exactly one ending call, including a failed call.
    try {
        float *output[2] = {out_left, out_right};
        owner->stretch.flush(output, static_cast<int>(frames), 1.0f);
        return finite_output(out_left, out_right, frames) ? 0 : 3;
    } catch (const std::bad_alloc &) {
        return 2;
    } catch (const std::exception &) {
        return 3;
    } catch (...) {
        return 3;
    }
}

extern "C" void flitzis_signalsmith_probe_destroy(FlitzisSignalsmithProbe *owner) noexcept {
    delete owner;
}
