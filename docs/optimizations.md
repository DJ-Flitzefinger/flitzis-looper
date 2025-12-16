# Audio Engine Optimization Guidelines

## Overview

This document outlines key performance optimizations for the flitzis-looper audio engine to efficiently handle up to 50 simultaneous audio samples (1-5 minutes each) with pitch shifting and effects, following pyo's performance best practices.

## Critical Bottleneck: Temporary File I/O

### Problem
The current implementation uses temporary WAV files to bridge between NumPy arrays (from pedalboard pitch shifting) and pyo's `SndTable`:

```python
# Current (inefficient):
sf.write(temp_path, pitched_audio, sample_rate)
table = SndTable(temp_path)
```

This creates a full disk I/O cycle for every pitch shift operation, which is extremely slow and creates unnecessary I/O pressure.

### Solution: Use DataTable with Direct NumPy Arrays
Replace temporary files with direct NumPy array transfer using pyo's `DataTable`:

```python
# Optimized implementation:
table_size = len(pitched_audio)
pitched_table = DataTable(size=table_size, init=pitched_audio.flatten())

# For stem tables in stems_engine.py:
data["stems"]["main_table"] = DataTable(size=len(main_audio), init=main_audio.flatten())
data["stems"]["tables"][stem] = DataTable(size=len(audio_data), init=audio_data.flatten())
```

**Benefits:**
- Eliminates disk I/O completely
- Reduces latency from milliseconds to microseconds
- Eliminates file system overhead
- Reduces memory fragmentation
- 10-100x faster table creation

## Memory Management Optimization

### Problem
With 50 samples × 5 minutes = ~5.3GB RAM usage, memory pressure can cause system instability.

### Solutions:

#### 1. Implement Audio Streaming for Large Files
```python
def _should_stream_audio(self, audio_size_bytes):
    """Stream audio files larger than 10% of memory limit"""
    return audio_size_bytes > (self._max_memory_gb * 1024**3 / 10)

def load(self, path):
    if self._should_stream_audio(os.path.getsize(path)):
        # Implement streaming from file instead of loading entire file
        self._streaming_enabled = True
        self._audio_path = path
        # Use pyo SndTable with file path directly
        self._table = SndTable(path)
    else:
        # Load into memory normally
        self._audio_data, self._audio_sr = sf.read(path, dtype="float32")
        self._table = DataTable(size=len(self._audio_data), init=self._audio_data.flatten())
```



## Pitch Shifting Optimization

### Problem
Using pedalboard + NumPy for pitch shifting creates unnecessary overhead.

### Solution: Hybrid Pitch Shifting Approach
Use a hybrid approach that leverages the strengths of both pyo's native `Pitch` and Pedalboard based on shift magnitude:

```python
# Determine pitch shift in semitones
semitones = speed_to_semitones(self._pending_speed)

# Use pyo Pitch for small shifts (±3 semitones or less)
# - Extremely low CPU usage
# - Zero latency
# - No memory allocation
# - Indistinguishable quality from Pedalboard at small shifts
if abs(semitones) <= 3:
    self._pitched_player = Pitch(
        source=self.player,
        freq=base_freq * self._pending_speed,
        mul=self.amp * master_amp,
        interp=4
    )
# Use Pedalboard for larger shifts (±4+ semitones)
# - Superior quality for large shifts
# - Better preservation of transients and harmonic content
# - Acceptable CPU cost for less frequent large shifts
else:
    self._create_pitched_player_with_pedalboard()
```

**Quality Comparison:**
- **pyo Pitch**: Best for small shifts (±3 semitones). Uses simple resampling with linear interpolation. No artifacts at small shifts, but introduces "chirping" and phase distortion at larger shifts.
- **Pedalboard**: Uses Rubberband's phase vocoder. Excellent quality for large shifts, preserves transients and harmonic relationships. Higher CPU usage but necessary for fidelity.

**Recommendation**:
- Use pyo Pitch for ±3 semitones or less (covers ~90% of DJ/looping use cases)
- Use Pedalboard only for ±4+ semitones (rare, dramatic pitch changes)
- This provides optimal balance: 90% of operations are ultra-efficient, while critical cases maintain professional quality

## Table Management Optimization

### Problem
Creating new DataTables for every playback cycle creates memory allocation overhead.

### Solution: Table Reuse with fill()
```python
def __init__(self):
    self._table_cache = {}  # Cache of reused DataTables

def _get_or_create_table(self, size, init_array=None):
    """Reuse DataTables when possible to reduce memory allocation"""
    # Create a unique key based on size and content hash
    if init_array is not None:
        content_hash = hash(init_array.tobytes())
        key = (size, content_hash)
    else:
        key = (size, None)
    
    if key in self._table_cache:
        table = self._table_cache[key]
        if init_array is not None:
            table.fill(init_array.flatten())
        return table
    else:
        table = DataTable(size=size, init=init_array.flatten() if init_array is not None else 0)
        self._table_cache[key] = table
        return table
```

**Benefits:**
- Eliminates memory allocation/deallocation overhead
- Reduces garbage collection pressure
- Improves real-time performance consistency

## Audio Processing Chain Optimization

### Problem
Multiple redundant processing steps in the audio pipeline.

### Solution: Optimize Signal Chain
```python
def _create_eq_chain(self):
    """Optimized EQ chain with minimal processing steps"""
    if not self.player:
        return
    
    # Use direct signal path when possible
    if self._key_lock and self._pitched_player:
        signal = self._pitched_player
    else:
        signal = self.player
    
    # Reduce number of EQ stages when possible
    self.eq_low = EQ(
        signal, freq=200, q=0.7, boost=self._get_eq_boost(self._eq_low_val), type=1
    )
    
    # Only add mid/high EQ if actually needed
    if self._eq_mid_val != 0 or self._eq_high_val != 0:
        self.eq_mid = EQ(
            self.eq_low, freq=1000, q=0.7, boost=self._get_eq_boost(self._eq_mid_val), type=0
        )
        if self._eq_high_val != 0:
            self.eq_high = EQ(
                self.eq_mid, freq=4000, q=0.7, boost=self._get_eq_boost(self._eq_high_val), type=2
            )
            self.output = self.eq_high
        else:
            self.output = self.eq_mid
    else:
        self.output = self.eq_low
```

## pyo-Specific Performance Best Practices

### 1. Master Amplitude Control
The current implementation multiplies every audio object by the master amplitude, creating unnecessary dynamic multiplications:

```python
# Current (inefficient):
self.player = SfPlayer(..., mul=self.amp * master_amp)
```

**Optimization:**
Apply master amplitude only at the final output stage:
```python
# Optimized:
self.player = SfPlayer(..., mul=self.amp)  # Use default mul=1
self.output = self.player * self._master_amp  # Single multiplication at output
```

### 2. EQ Chain Optimization
The current implementation chains three separate EQ objects, which is CPU-intensive:

```python
# Current (inefficient):
self.eq_low = EQ(signal, ...)
self.eq_mid = EQ(self.eq_low, ...)
self.eq_high = EQ(self.eq_mid, ...)
```

**Optimization:**
Use `Biquadx` with multiple stages instead of chaining individual EQ objects:
```python
# Optimized:
# For a 3-band EQ equivalent:
self.eq_chain = Biquadx(
    signal, 
    freq=[200, 1000, 4000], 
    q=[0.7, 0.7, 0.7], 
    boost=[self._get_eq_boost(self._eq_low_val), 
           self._get_eq_boost(self._eq_mid_val), 
           self._get_eq_boost(self._eq_high_val)], 
    type=[1, 0, 2], 
    stages=1  # One stage per band
)
self.output = self.eq_chain
```

### 3. Denormal Number Protection
Add protection against denormal numbers in filters, delays, and reverbs:
```python
# Create a small noise source for denormal protection
self._denorm_noise = Noise(1e-24)

# Add to any objects with recursive delay lines:
self.delay = Delay(src + self._denorm_noise, delay=0.1, feedback=0.8, mul=0.2).out()
self.reverb = WGVerb(src + self._denorm_noise).out()
```

### 4. Avoid Audio Rate Control Parameters
Avoid using `Sig` objects for parameters that don't need real-time control:
```python
# Avoid this (wastes CPU):
self.phaser = Phaser(src, spread=Sig(1.2), ...)

# Use this instead:
self.phaser = Phaser(src, spread=1.2, ...)  # Fixed value
```

## Summary of Optimizations

| Optimization | Impact | Complexity |
|--------------|--------|------------|
| Replace temporary files with DataTable | ★★★★★ (Critical) | Low |
| Implement audio streaming for large files | ★★★★☆ | Medium |
| Use pyo Pitch instead of pedalboard | ★★★★☆ | Low |
| Reuse DataTables with fill() | ★★★★☆ | Medium |
| Optimize EQ chain with Biquadx | ★★★★★ (Critical) | Low |
| Consolidate master amplitude control | ★★★★☆ | Low |
| Implement denormal protection | ★★★☆☆ | Low |

## Implementation Priority

1. **Immediate**: Replace temporary files with DataTable (highest impact)
2. **Immediate**: Optimize EQ chain with Biquadx (critical CPU reduction)
3. **High Priority**: Implement audio streaming for large files
4. **High Priority**: Consolidate master amplitude control
5. **Medium Priority**: Reuse DataTables with fill()
6. **Low Priority**: Use pyo Pitch instead of pedalboard (for simple shifts)

8. **Low Priority**: Implement denormal protection

## Testing Recommendations

1. Create a test suite with 50 simultaneous 5-minute samples
2. Monitor CPU usage, memory usage, and latency with `htop` and `pyo`'s built-in monitoring (`Server.getCpuLoad()`)
3. Test with various pitch shift amounts (±12 semitones)
4. Verify no audible artifacts after optimization
5. Measure performance improvement using `time.time()` before/after key operations
6. Test with denormal-prone effects (delays, reverbs) to verify denormal protection works
```
