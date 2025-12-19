# Ring Buffer Messaging Specification

## Purpose
To enable efficient, lock-free communication between audio processing threads and control threads using a fixed-size circular buffer.
## Requirements
### Requirement: Lock-free Ring Buffer
The system SHALL provide a lock-free ring buffer for inter-thread messaging between audio engine and control threads, with a maximum capacity of 1024 messages and fixed-size message structs.

#### Scenario: Single message transfer
- **WHEN** a message is written by the producer
- **THEN** it is readable by the consumer
- **AND** the buffer head and tail pointers are correctly updated

### Requirement: Real-time Safety
The system SHALL ensure no heap allocations, no Python GIL acquisition, and no blocking operations during audio thread message processing.

#### Scenario: Audio thread message processing
- **WHEN** a message is received in the audio thread
- **THEN** no heap allocations occur
- **AND** the Python GIL is not acquired
- **AND** no blocking operations are performed

### Requirement: Ring Buffer Integration
The system SHALL provide a lock-free ring buffer for inter-thread messaging between audio engine and control threads, with a maximum capacity of 1024 messages and fixed-size message structs.

#### Scenario: Buffer capacity limit
- **WHEN** the buffer is filled to its 1024-message capacity
- **THEN** subsequent writes are rejected with an error code
- **AND** the producer is notified without blocking

### Requirement: Error Handling
The system SHALL handle ring buffer full/empty conditions gracefully.

#### Scenario: Full buffer handling
- **WHEN** the ring buffer is full and Python attempts to send a message
- **THEN** the message is dropped
- **AND** an appropriate error code is returned
- **AND** no panic occurs in the audio thread

#### Scenario: Empty buffer handling
- **WHEN** the audio thread attempts to read from an empty ring buffer
- **THEN** no message is returned
- **AND** the thread continues processing normally
- **AND** no blocking occurs

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
