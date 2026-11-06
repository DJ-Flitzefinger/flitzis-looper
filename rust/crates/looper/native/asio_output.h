#ifndef FLITZIS_ASIO_OUTPUT_H
#define FLITZIS_ASIO_OUTPUT_H

#include <stdint.h>

// The public ABI contains only fixed-width scalars and borrowed pointers. SDK
// C++ interfaces, long, packing and COM apartment ownership stay in the bridge.
struct FlAsioSession;
struct FlAsioInfo {
    double sample_rate;
    int32_t output_channels;
    int32_t min_frames;
    int32_t max_frames;
    int32_t preferred_frames;
    int32_t granularity;
    int32_t prepared_frames;
    int32_t input_latency_frames;
    int32_t output_latency_frames;
    int32_t output_ready;
};
struct FlAsioChannel {
    int32_t index;
    int32_t sample_type;
    int32_t group;
    char name[32];
};
struct FlAsioTime {
    uint64_t system_ns;
    uint64_t sample_position;
    double sample_rate;
    uint32_t flags;
    int32_t direct_process;
    uint32_t latency_revision;
    uint32_t notifications;
};
struct FlAsioStatus {
    uint32_t notifications;
    int32_t callback_error;
    uint32_t in_flight;
    uint32_t latency_revision;
    double last_notified_rate; // zero means absent or explicitly unknown
    uint64_t rate_notice_count;
};
// Physical close and driver-global rate restoration are independent receipts.
// flags: baseline=1, attempted=2, committed=4, conflicting notice=8.
// disposition: unchanged=0, committed=1, restored=2, conflict=3,
// unknown=4, restore-failed=5. Queries never imply an atomic external lease.
struct FlAsioRateTransaction {
    double original_rate;
    double target_rate;
    double observed_rate;
    double restore_before_rate;
    double restore_after_rate;
    int32_t set_status;
    int32_t query_status;
    int32_t restore_set_status;
    int32_t restore_query_status;
    int32_t disposition;
    uint32_t flags;
};
// A copied block returns its exclusive engine-frame end, or zero on refusal.
typedef uint64_t (*FlAsioRender)(void*, void*, void*, int32_t, int32_t, int32_t,
                                 const FlAsioTime*);
typedef void (*FlAsioSubmitted)(void*, uint64_t, uint64_t);
constexpr uint32_t fl_asio_detached = 128;
typedef void (*FlAsioInvalidate)(void*, uint32_t, uint32_t);
typedef void (*FlAsioEnumerate)(void*, const uint16_t*, const uint16_t*);

extern "C" {
int32_t fl_asio_enumerate(void*, FlAsioEnumerate) noexcept;
int32_t fl_asio_new(FlAsioSession**) noexcept;
int32_t fl_asio_open(FlAsioSession*, const uint16_t*) noexcept;
int32_t fl_asio_info(FlAsioSession*, FlAsioInfo*) noexcept;
int32_t fl_asio_channel(FlAsioSession*, int32_t, FlAsioChannel*) noexcept;
int32_t fl_asio_can_rate(FlAsioSession*, double) noexcept;
int32_t fl_asio_set_rate(FlAsioSession*, double) noexcept;
int32_t fl_asio_commit_rate(FlAsioSession*) noexcept;
int32_t fl_asio_abort_candidate_rate(FlAsioSession*) noexcept;
int32_t fl_asio_rate_transaction(FlAsioSession*, FlAsioRateTransaction*) noexcept;
int32_t fl_asio_prepare(FlAsioSession*, int32_t, int32_t, int32_t,
                        void*, FlAsioRender, FlAsioSubmitted, FlAsioInvalidate) noexcept;
int32_t fl_asio_start(FlAsioSession*) noexcept;
// Success verifies Stop, in-flight zero, callback detachment, Dispose and COM
// Release, in that order. Failure leaves the same session available for retry.
int32_t fl_asio_close(FlAsioSession*) noexcept;
// Only succeeds after verified close. A failed close never deletes the session.
int32_t fl_asio_delete(FlAsioSession*) noexcept;
void fl_asio_status(FlAsioSession*, FlAsioStatus*) noexcept;
void fl_asio_pump_messages() noexcept;
}
#endif
