## ADDED Requirements

### Requirement: Exact Unicode audio picker results
The audio picker SHALL return native Unicode paths without ANSI decoding,
normalization, character replacement, renamed copies or aliases. It SHALL retain
the opening PAD and consume a selected result once on the UI/control thread.
Cancellation and picker errors SHALL leave source assignments/settings unchanged;
errors SHALL use existing per-PAD load diagnostics before any asset reservation.

The App SHALL own at most one picker helper process and its fresh IPC file.
Cancellation SHALL be marked before exact-owned process termination and SHALL
permanently prevent selected-result adoption, including late successful results.
Old ownership SHALL remain until actual process termination is observed. The App
SHALL validate a bounded closed UTF-8 result schema after zero process exit before
selected-path forwarding. Child stdio SHALL NOT use unread blocking pipes.
Only the exact owned IPC file may be removed, after the child is terminal.

The helper SHALL import neither the App/controller nor its audio backend. It MAY
use the installed Unicode native picker internally; cancellation SHALL NOT depend
on that library's synchronous in-process kill/future race. Normal UI polls SHALL
remain nonblocking. Controller shutdown settlement SHALL keep progressing while
the picker retires, and picker/exit-hook errors SHALL NOT skip controller teardown
after its normal settlement gate.

#### Scenario: A typographic apostrophe or non-ANSI character is selected
- **WHEN** the user selects an audio file with U+2019, CJK, non-BMP or combining characters
- **THEN** its exact Unicode path reaches ordinary load admission once
- **AND** the render loop does not attempt narrow filesystem-string decoding

#### Scenario: Close, cancel or another PAD wins before native dialog startup
- **WHEN** the owned picker is cancelled before its native window exists
- **THEN** its exact process is terminated and ownership retained until terminal
- **AND** no late result can load the old or a newly selected PAD
- **AND** normal controller settlement continues while closing
