#ifndef FLITZIS_SIGNALSMITH_PROBE_H
#define FLITZIS_SIGNALSMITH_PROBE_H

#include <stdint.h>

#ifdef __cplusplus
#define FLITZIS_PROBE_NOEXCEPT noexcept
extern "C" {
#else
#define FLITZIS_PROBE_NOEXCEPT
#endif

typedef struct FlitzisSignalsmithProbe FlitzisSignalsmithProbe;

typedef struct {
    uint32_t version_major;
    uint32_t version_minor;
    uint32_t version_patch;
    uint32_t channels;
    uint32_t sample_rate;
    uint32_t block_samples;
    uint32_t interval_samples;
    uint32_t input_latency;
    uint32_t output_latency;
    uint32_t split_computation;
    uint32_t seed;
} FlitzisSignalsmithInfo;

/* Status: 0 success, 1 invalid input, 2 allocation, 3 exception, 4 terminal/poisoned.
 * No function retains PCM pointers. Processing exceptions/nonfinite outputs poison
 * the owner; invalid preflight arguments do not mutate it.
 * One Rust non-Send/non-Sync owner is the only caller and always destroys it.
 */
int32_t flitzis_signalsmith_probe_create(
    double pitch, FlitzisSignalsmithInfo *info, FlitzisSignalsmithProbe **owner
) FLITZIS_PROBE_NOEXCEPT;
int32_t flitzis_signalsmith_probe_process(
    FlitzisSignalsmithProbe *owner,
    const float *left, const float *right,
    float *out_left, float *out_right, uint32_t frames
) FLITZIS_PROBE_NOEXCEPT;
int32_t flitzis_signalsmith_probe_flush(
    FlitzisSignalsmithProbe *owner, float *out_left, float *out_right, uint32_t frames
) FLITZIS_PROBE_NOEXCEPT;
void flitzis_signalsmith_probe_destroy(FlitzisSignalsmithProbe *owner) FLITZIS_PROBE_NOEXCEPT;

#ifdef __cplusplus
}
#endif
#undef FLITZIS_PROBE_NOEXCEPT
#endif
