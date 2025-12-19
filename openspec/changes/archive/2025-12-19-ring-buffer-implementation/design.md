# Ring Buffer Implementation Design

## Architecture Overview

This design implements a lock-free message passing system between Python and the real-time audio thread using the `rtrb` crate. The system follows a Single Producer, Single Consumer (SPSC) pattern where:

- **Producer**: Python thread via PyO3 FFI
- **Consumer**: Real-time audio thread via CPAL callback
- **Transport**: Lock-free ring buffer with zero allocations

## Key Design Principles

### 1. Real-Time Safety
- No heap allocations in audio thread
- No blocking operations in audio thread
- No Python GIL acquisition in audio thread
- Pre-allocated message buffers

### 2. Efficient Message Format
- Rust enums as wire format (no serialization)
- Arc-wrapped data for large payloads (samples)
- Minimal message size for common operations

### 3. Error Handling
- Graceful degradation when ring buffer is full
- Clear error boundaries between threads
- No panics in audio thread

## Technical Implementation

### Message Protocol
```rust
pub enum AudioMessage {
    Ping,
    Pong,
    // Future extensions
    Stop,
    SetVolume(f32),
    LoadSample { id: usize, data: Arc<Vec<f32>> },
    PlaySample { id: usize, velocity: f32 },
}
```

### Ring Buffer Integration
- Using `rtrb::RingBuffer` for SPSC communication
- Fixed capacity (1024 messages)
- Non-blocking push/pop operations
- Error handling for full/empty conditions

### Thread Separation
1. **Python Thread**:
   - Handles I/O operations (file loading)
   - Memory allocations
   - Message construction
   - Ring buffer production

2. **Audio Thread**:
   - Consumes messages from ring buffer
   - Updates internal state
   - Renders audio samples
   - No allocations or blocking

## Implementation Strategy

### Phase 1: Core Infrastructure
1. Add `rtrb` dependency
2. Define message enum
3. Implement ring buffer creation
4. Integrate with audio engine

### Phase 2: Ping/Pong Messaging
1. Implement ping/pong message handling
2. Add Python API for sending messages
3. Verify message delivery

### Phase 3: Testing and Validation
1. Rust unit tests
2. Python integration tests
3. Real-time constraint verification

## Trade-offs and Considerations

### Buffer Size
- Larger buffers reduce chance of message loss
- Larger buffers increase latency
- Starting with 1024 messages as reasonable default

### Message Loss Handling
- Dropping messages when buffer is full
- Future enhancement: Error reporting to Python

### Memory Management
- Arc for shared data ownership
- Pre-allocated storage where possible
- Clear ownership boundaries