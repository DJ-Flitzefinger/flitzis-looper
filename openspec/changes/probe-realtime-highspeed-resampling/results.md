## Actual result: candidate rejected

The distinct HighSpeed prerequisite executed successfully as two technical tests
(0.88s after a 2m50 optimized compile). It did **not** qualify the audio candidate:
all four actual nominal and unchanged strict kick cases failed. No broader matrix
or productive backend/profile/guard change follows this result.

Actual pre-signal queries matched the predeclared engine3, stereo, q1/p2 and
P4096/N2048. Both caller partitions produced identical complete Raw PCM. Canonical
inputs, original source identity and the interior196608-frame old Live prefix
were unchanged. Every real input frame was submitted once, finalization occurred
once, and complete output drained to actual available==-1.

| Kick position | Actual nominal q10/q50/q90 error, frames | Complete Raw frames | Removed original energy at nominal start |
| --- | --- | --- | --- |
| Origin | +338 / +299 / +441 | 270580 | 0.0020658363908992706 |
| Interior | -332 / +130 / +312 | 401632 | 0 |

Each row was reproduced under both512 and irregular callers. Timing exceeds the
unchanged88-frame/2ms limit; origin removed energy exceeds the unchanged0.001
limit. Fixed nominal last20ms under both denominators, extra Raw suffix and actual
drained last20ms energy were all zero; the complete nominal windows were available.
The original Raw peak was retained in both complete nominal windows. This does
not imply the unchanged strict firstSound gate retained it: the interior gate at
N+event loses the original peak and0.29910171366439126 of original energy.

The origin timing-only diagnostic interval[249,386] has empty joint retention
interval[249,-10]. Interior timing-only[42,-244] is itself empty, with minimax
q10/q50 lower bound231 frames; its joint interval[42,-506] is empty too. These are
diagnostics, never selected compensation values.

Own fresh HS unity q1/p1 queried P=N2048 and passed both callers: nominal q errors
and peak offset all zero, complete caller PCM equal, max absolute PCM error
0.0041582584381103516 and RMS0.0003226343862403647. Unity timing is not a
sample-exact waveform promise or a musical/timbre qualification.

## Compatibility and resource scope

The separately HC-filtered rerun passed two technical tests(1.44s, cached compile
0.19s). All46 nonvolatile JSON rows, including eight complete Raw/canonical
captures and every original diagnostic metric, exactly matched the retained
probe-realtime-mapped-stretcher log; tolerance zero. Only process/executable/IAT
identity fields were excluded from equality. Both actual loaded DLL hash and
function binding were independently reverified. The old scorer and following HC
test bodies remain byte-for-byte unchanged.

The pre-signal owned synthetic array admission uses a256MiB budget. It includes
actual original/shifted/canonical capacities and vector headers, two conservative
full Raw capacity slots, process blocks, both4096B staging/drain buffers, and all
scorer temporaries summed conservatively as128R+56C+1104B, including temporaries
that do not overlap. Allocator metadata, JSON diagnostics, Native internal RAM,
callback and whole-process/resource216 acceptance are explicitly excluded.
The maximum pre-signal array bound was187861040B; the conservative bound using
actual extents/capacities peaked at87604320B. Neither is a measured process RSS.

The DLL remained298496B/SHA256
4a8d323cc8c40d16413bbffe459811f93ebb5f7249b9e830bf4d11c62e4feb6d. Its exact library
semver/source identity remains unavailable; v4.0.0 is the primary mechanism
reference, not a proven binary source version. Productive source is unchanged:
the installed34469376B Release remains SHA256
581d587e4d2bddcb27e08998fb74cfd3f010250ec3be608e00807e15bf031d48, with twelve
reflection-only API families/editorAPI3 checked without constructing an engine.
The compatible preceding productive Release/build/preflight evidence is reused.

## Evidence and remaining gates

Owned evidence: workspace/test-artifacts/realtime-highspeed-prerequisite-20261022/
highspeed-first.log, highspeed-all-records.json, highspeed-summary.json,
hc-parity-run.log, hc-parity.json and installed-api-reuse.json. The unchanged
offline-owner regression passed one test(0.41s, cached compile0.30s). Workspace
all-target Clippy passed42.08s in dev and41.43s in Release, with no diagnostic in
either changed Rust module. Release retained the preceding97lib/132lib-test
(57duplicates)/197example warnings; dev reported two additional duplicate
diagnostics(134/59). Cargo formatting, both included-module rustfmt checks,
strict validation of this and probe-realtime-mapped-stretcher, and whitespace
checks passed. One independent NONAUTHOR review returned CLEAR for staged tree
d49b925aa15c30cd00bcd9c9f624dfa1a83a57ee, seven files; no actionable findings.
It expressly covers only the final task checkbox and this factual review note
after that reviewed tree, without further source changes.
No App/device/listening run occurred.

This cfg(test)-only result closes neither automatic map/B2/default cutover nor
Tone/Cents/timbre, dynamic pitch, both KEYLOCK states,37 rates/Total-23…+24,
retainedFuture/FIFO/Mixer/S/E/T/deadline/start modes, resource216 or Human gates.
The unlimited FULL Goal remains active, preserving all original/extensions and
later Human requirements. Stop before full Rust-port/Slice8.
