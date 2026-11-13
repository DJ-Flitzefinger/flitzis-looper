## ADDED Requirements

### Requirement: Explicit finite audio workload report
The Settings surface SHALL offer an explicit one-shot capture and clipboard copy
of the existing App's audio workload. Capture SHALL use the ordinary control tick
and existing nonblocking Native snapshot API, serializing PAD requests without
starting workers, devices, playback or persistent project writes. Capture SHALL
finish or stop polling within five seconds of control polling, permit cancellation,
and cancel on application close. A subsequent explicit capture SHALL replace the
retained report. Unattempted and unfinished PADs SHALL be identified.

The report SHALL distinguish the requested buffer size from observed callback
frames, control intent from effective Native state, and serial observations from a
simultaneous snapshot. Callback geometry MAY be available without an accepted
voice; effective voice values SHALL require a fresh current acknowledged source.
Unavailable, stale, ambiguous and superseded observations SHALL NOT imply accepted
voice state, nonzero PCM, acoustic cause, CPU load, quality or deadline acceptance.
The report SHALL omit source paths and binding dictionaries and identify actual
limits of artifact/build provenance and external background observations.

#### Scenario: Manual source without accepted voice timing
- **WHEN** the user captures a workload containing a manually timed active PAD
- **THEN** the report includes the serviced callback geometry independently
- **AND** effective voice values remain unavailable with their reason
- **AND** the existing source and playback continue unchanged

#### Scenario: Snapshot does not complete
- **WHEN** a pending request is superseded, times out or the user closes the App
- **THEN** capture retains a partial report and stops polling
- **AND** it does not retry that request or create a background observer
