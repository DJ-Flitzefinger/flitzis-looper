## ADDED Requirements

### Requirement: CUDA Worker Setup Verifies A Known Profile Before Paired Delivery
Setup SHALL accept only the legacy CPU profile or the known CUDA-auto profile,
each bound to its specific shipped source project, worker files and lock. CUDA
dependency markers SHALL be evaluated against worker Windows/AMD64/Python3.12.13,
not against the App interpreter. Setup SHALL NOT trust a manifest merely because
its supplied hashes agree with arbitrary supplied files.

Package download and installation SHALL NOT have a total elapsed-time deadline.
Slow Internet alone SHALL NOT cancel the operation or discard reusable download
bytes. Recovery SHALL retain/resume available bytes and verify the pinned hash
before use. Explicit user interruption SHALL remain possible. The separate local
environment-inspection and inference/process safety limits SHALL remain in force.

A staged installation SHALL be fully verified before atomic pointer publication.
Explicit inactive staging SHALL retain the new verified installation directory
and return its configuration directly, without reading or writing `current.json`
or requiring capability from the currently loaded Native. An absent pointer SHALL
remain absent. Separate activation SHALL accept only a validated non-reparse
direct child of the installation root, reverify its known manifest/files and
checkpoint, then require CUDA Native execution capability2 immediately before
atomic pointer replacement. Legacy CPU activation SHALL remain usable with its
old compatible Native. Unknown profiles and modified staged sources SHALL NOT
change the pointer.
Failure SHALL retain the previous pointer and verified installation. Actual App
delivery SHALL pair the selected pointer with a Native version capable of that
protocol under the existing closed-App/held-Native guard. Rollback SHALL restore
both pointer and Native pair. One active CUDA-capable environment SHALL support
CPU fallback; the old CPU installation MAY be retained temporarily for rollback.

#### Scenario: A staged CUDA runtime fails verification
- **WHEN** package, lock, worker or runtime inspection differs from the known profile
- **THEN** setup SHALL reject the staged runtime before publishing its pointer
- **AND** the previous CPU installation and its source evidence SHALL remain intact

#### Scenario: CUDA is prepared while the previous Native is still loaded
- **WHEN** explicit inactive staging successfully verifies the new CUDA installation
- **THEN** it SHALL return that new installation configuration without consulting the active pointer
- **AND** the previous pointer bytes SHALL remain untouched or its absence SHALL remain unchanged
- **AND** later activation with incompatible Native SHALL fail without a pointer change

#### Scenario: A large CUDA package downloads over slow Internet
- **WHEN** a permitted download or package installation continues for a long time
- **THEN** elapsed wall-clock time alone SHALL NOT terminate it
- **AND** any retained partial download SHALL remain eligible for verified resume instead of elapsed-time cleanup
