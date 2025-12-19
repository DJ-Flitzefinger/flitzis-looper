# Ring Buffer Messaging Specification

## Why
To enable efficient, lock-free communication between audio processing threads and control threads using a fixed-size circular buffer.

## What Changes
- Implemented ring buffer for message passing
- Added producer/consumer APIs
- Ensured thread-safety with atomic operations
- Added ping/pong messaging protocol using Rust enum
- Implemented error handling for full/empty buffer conditions
- Enforced real-time safety: no allocations, no GIL acquisition, no blocking

### Requirement: Ring Buffer Integration
The system must provide a lock-free ring buffer for inter-thread messaging between audio engine and control threads, with a maximum capacity of 1024 messages and fixed-size message structs.

## Design
- Fixed-size buffer with head/tail pointers
- Single producer, single consumer model
- Memory barriers for cross-thread visibility

## API
- `ping()` (Python API)
- `receive_msg()` (Python API)

## Constraints
- Maximum buffer size: 1024 messages
- Messages are fixed-size structs
- No dynamic allocation during runtime
- Message sending latency must be under 1 microsecond
- No heap allocations in audio thread
- No Python GIL acquisition during message processing
- No blocking operations in audio thread

## Test Cases
- Write/read single message
- Fill buffer and drain
- Concurrent access (simulated)
- Overflow detection
- Underflow detection
- Ping message sent from Python and received in audio thread
- Pong message sent from audio thread and received in Python
- Buffer full condition: message dropped with error code
- Buffer empty condition: no blocking, thread continues
- Real-time safety: no allocations or GIL acquisition during message processing
- Message sending latency under 1 microsecond
