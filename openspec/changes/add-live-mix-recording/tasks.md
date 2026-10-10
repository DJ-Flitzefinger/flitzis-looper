## X11-RECORD-CAPTURE — pending

- [ ] Add focused native session/frame-bound capture state and control API at the existing segmented scheduled render seam; reserve fixed block/ring/feedback/retirement capacity off callback and prohibit RT allocation/I/O/locks/GIL/encoding/logging.
- [ ] Add non-RT contiguous float32 spool draining and exact stop/end-frame acknowledgement, session fencing, terminal failure and shutdown stop/drain/retire lifecycle with truthful incomplete recovery state.
- [ ] Prove productive exact mixed first/end samples across irregular callbacks/in-buffer schedule events and existing DSP/master/stem/KEYLOCK paths with an independent oracle; measure actual fixed capture peaks and exercise overflow/slow writer/disk/stream/teardown failure while playback continues.
- [ ] Complete impact-appropriate Rust/FFI checks, docs, official strict validation and independent final source review; publish only this bounded API/worker result, without claiming the complete performer feature.

## X11-RECORD-FORMAT — pending after CAPTURE

- [ ] Add validated persisted WAV/FLAC/MP3 and explicit per-format settings/defaults through existing settings/persistence; reuse suitable FFmpeg discovery and preserve unrelated state on malformed recording fields.
- [ ] Establish writer/selected-codec/output-geometry ready permit before launch; implement verified encode/close/atomic publish, RF64 long WAV, explicit saturation/clipping metadata and MP3 delay/padding verification without normalization or fallback.
- [ ] Implement exclusive unique repo/record paths and /record/ Git ignore, recorder-owned incomplete recovery/no overwrite and stop/drain/finalize shutdown behavior.
- [ ] Verify actual files independently for codec/quality/rate/channels/extent, real long RF64 handling and bounded memory; exercise empty/corrupt/incomplete/encoder/disk/shutdown/reopen failures, then docs/strict/independent review/publication.

## X11-RECORD-CONTROL — pending after CAPTURE and FORMAT

- [ ] Reuse existing active/remembered GlobalPlaybackController selection, source/accepted timing/effective loops, residency wait and native scheduler/batch; integrate writer-ready capture and all pads in one all-or-none start transaction and acknowledgement.
- [ ] Add RECORD left of START/STOP with deliberate gap and shared suitable button/gesture helper; use single click edges, truthful states and defined repeat/busy/error/empty behavior; keep standard START/STOP independent.
- [ ] Prove productive first captured frame equals native group start, each pad begins its effective loop start, MULTI LOOP ON/OFF and stem combinations survive, and stale/queue/readiness/capacity failure admits neither partial group nor take.
- [ ] Complete controller/native/UI/file integration and impact checks, docs, official strict validation and independent nonauthor review/publication; retain later R2/B7/V0 integration and final Human GUI/device/recording/listening gates OPEN.
