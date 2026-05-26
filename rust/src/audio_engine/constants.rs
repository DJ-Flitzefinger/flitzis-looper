//! Audio engine configuration constants and limits.

/// Number of sample banks available.
pub const NUM_BANKS: usize = 6;

/// Size of the sample grid (GRID_SIZE x GRID_SIZE).
pub const GRID_SIZE: usize = 6;

/// Total number of pads in the grid.
pub const NUM_PADS: usize = GRID_SIZE.pow(2);

/// Total number of sample slots (pads × banks).
pub const NUM_SAMPLES: usize = NUM_PADS * NUM_BANKS;

/// Maximum number of voices that can be active simultaneously.
pub const MAX_VOICES: usize = 32;

/// Maximum number of accepted absolute-frame scheduler events.
pub const MAX_SCHEDULED_EVENTS: usize = 1024;

/// Minimum playback speed multiplier (50%).
pub const SPEED_MIN: f32 = 0.5;

/// Maximum playback speed multiplier (200%).
pub const SPEED_MAX: f32 = 2.0;

/// Minimum volume level (silence).
pub const VOLUME_MIN: f32 = 0.0;

/// Maximum volume level (100%).
pub const VOLUME_MAX: f32 = 1.0;

/// Minimum per-pad Gain/Trim in dB.
pub const PAD_GAIN_DB_MIN: f32 = -12.0;

/// Maximum per-pad Gain/Trim in dB.
pub const PAD_GAIN_DB_MAX: f32 = 12.0;

/// Default per-pad Gain/Trim in dB.
pub const PAD_GAIN_DB_DEFAULT: f32 = 0.0;

/// Per-pad Gain/Trim smoothing time in milliseconds.
pub const PAD_GAIN_SMOOTH_MS: f32 = 10.0;

/// Minimum per-band EQ gain in dB.
///
/// This represents a DJ-style "Kill" position, and is mapped to a linear gain of 0.0.
pub const PAD_EQ_DB_MIN: f32 = -60.0;

/// Maximum per-band EQ gain in dB.
pub const PAD_EQ_DB_MAX: f32 = 6.0;
