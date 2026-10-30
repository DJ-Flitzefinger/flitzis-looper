## X11-RECORD-CAPTURE — bounded software complete

- [x] Add focused native session/frame-bound capture state and control API at the existing segmented scheduled render seam; reserve fixed block/ring/feedback/retirement capacity off callback and prohibit RT allocation/I/O/locks/GIL/encoding/logging.
- [x] Add non-RT contiguous float32 spool draining and exact stop/end-frame acknowledgement, session fencing, terminal failure and shutdown stop/drain/retire lifecycle with truthful incomplete recovery state.
- [x] Prove productive exact mixed first/end samples across irregular callbacks/in-buffer schedule events and existing DSP/master/stem/KEYLOCK paths with an independent oracle; measure actual fixed capture peaks and exercise overflow/slow writer/disk/stream/teardown failure while playback continues.
- [x] Complete impact-appropriate Rust/FFI checks, docs, official strict validation and independent final source review; publish only this bounded API/worker result, without claiming the complete performer feature.

The 2026-10-20 bounded checkpoint includes 134 combined Release regressions,
an isolated current default/max-pool allocation profile, normal installed Release
11-family API and actual Rust spool inspection, launcher/static/strict checks and
independent review. A spool is an unverified recovery candidate; selected formats,
public files, graceful recording finalization and atomic RECORD/group integration
remain below. Device/listening and whole-feature acceptance remain open.

## X11-RECORD-FORMAT — foundations implemented, integrated delivery pending

- [x] Add validated persisted WAV/FLAC/MP3 and explicit per-format settings/defaults through existing settings/persistence; preserve valid sibling preferences and unrelated state on malformed recording fields, with nonblocking visible/console diagnostics independent of unsafe-metadata save fences.
- [x] Reuse suitable FFmpeg discovery and bind actual selected encoder/container/output geometry in the codec-readiness component without changing producer semantics; check live native-writer/take integration separately below.
- [x] Establish writer/selected-codec/output-geometry ready permit before launch; implement verified encode/close/atomic publish, RF64 long WAV, explicit saturation/clipping metadata and MP3 delay/padding verification without normalization or fallback.
- [x] Implement exclusive unique repo/record paths and /record/ Git ignore, recorder-owned incomplete recovery/no overwrite and stop/drain/finalize shutdown behavior.
- [ ] Verify actual files independently for codec/quality/rate/channels/extent, real long RF64 handling and bounded memory; exercise empty/corrupt/incomplete/encoder/disk/shutdown/reopen failures, then docs/strict/independent review/publication.

The settings checkpoint covers 496 affected Python/headless-render/persistence
regressions and reviewed strict immutable option/recovery paths. Retaining an
options value is a foundation; prepared-take integration remains writer work.
Subsequent actual selected-codec tests cover 12 roundtrip variants. Six original
Native-ticket finalizer profiles cover WAV/FLAC/MP3, finite-payload mutation,
publication without post-link reads and committed custody-release uncertainty.
The actual long RF64 case independently verifies the complete payload exceeding
4 GiB, exact extent/hash/FileID/ds64 and declared process-memory limits. These
bounded foundations have independent review; full integrated delivery and device/
listening acceptance remain open. A fresh-child import-cycle regression exposed
by the later App integration is fixed, and the actual codec/finalizer cases pass
again. The initial failed runs and their limits remain in the test log.

## X11-RECORD-CONTROL — integration in progress

The Native next-admissible target component is independently bounded software
complete: 80 combined Release tests include exact nonzero-frame capture, real
deferral from requested F to effective E, immutable stop modes and actual PyO3
omitted/None arguments. The isolated capture resource profile and static/spec
checks pass. This component alone does not establish the atomic left group,
performer controller, GUI, installed package or device acceptance below.

- [ ] Reuse existing active/remembered GlobalPlaybackController selection, source/accepted timing/effective loops, residency wait and native scheduler/batch; integrate writer-ready capture and all pads in one all-or-none start transaction and acknowledgement.
- [x] Add RECORD left of START/STOP with deliberate gap and shared suitable button/gesture helper; use single click edges, truthful states and defined repeat/busy/error/empty behavior; keep standard START/STOP independent.
- [ ] Prove productive first captured frame equals native group start, each pad begins its effective loop start, MULTI LOOP ON/OFF and stem combinations survive, and stale/queue/readiness/capacity failure admits neither partial group nor take.
- [ ] Complete controller/native/UI/file integration and impact checks, docs, official strict validation and independent nonauthor review/publication; retain later R2/B7/V0 integration and final Human GUI/device/recording/listening gates OPEN.

The current App/UI checkpoint has 200 affected passing checks, including actual
ImGui glyph/rectangle/gesture observations across supported widths/scales and
explicit Native control mocks. Runtime polling, the fixed first-close deadline
and recording-before-audio teardown are integrated. The independent original
stream-failure/checked-quiescence component has 13 current Release regressions
and focused independent source review. A stale active-snapshot projection defect
was reproduced and fixed. Three actual generic-gate Controller/Thread/Native/WAV
routes pass again; the production GlobalPlayback composition, installed package,
whole-feature and Human/device gates remain open. These checkboxes do not claim
positive recording authority from mocked metadata or a checked close alone.
