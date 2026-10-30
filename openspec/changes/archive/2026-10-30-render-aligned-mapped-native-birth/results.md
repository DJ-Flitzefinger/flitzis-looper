## Actual retained-output result

The explicit original-owned Medium +7 experiment passes all eight declared
component cases. It emits real compensated PCM through the existing prepared
Native and its retained input/output FIFOs. Every emitted sample matches the
independent direct C-ABI stream exactly in this run; the fixed tolerance remains
3e-6. Both caller patterns produce bit-identical wet, dry and complete raw PCM.

The pre-signal observation is N=3078 and the independently declared compensation
is Cnom=N+511=3589. H=4096/4113, Dnew=7174/7191 and F=7685/7702. Actual input
receipts remain T+Cnom+j while logical output remains T+j. The five-span source
contains actual appended zeros; it does not wrap at the old four-span endpoint.

| Source-authored event | Native phase | Actual q10/q50 error, frames | Discarded original-control energy fraction |
| --- | ---: | ---: | ---: |
| First attack, frame 0 | 0 | +82 / +78 | 0.00004353319097117124 |
| First attack, frame 0 | 17 | +76 / +72 | 0.00005075063344398614 |
| Interior attack, frame 73728 | 0 | +84 / +83 | 0 |
| Interior attack, frame 73728 | 17 | +78 / +77 | 0 |

Each row is observed with both 512-frame and irregular callers. The worst q10/q50
error is 84 frames, or 1.75 ms at 48 kHz, within the unchanged 96-frame/2 ms budget.
All original raw peaks are retained. Both the actual fixed final 20 ms window and
the extra 506/489 raw frames have zero measured energy. The extended raw stream's
original prefix is bitwise identical to its four-span control; added raw suffix
energy is zero. Conservative original-control denominators also pass, so appended
silence has not diluted the discard or tail criteria.

## Verification and limits

The explicit test and independent five-span reference invariant pass. The initial
compile stopped before test execution because one test-only json! diagnostic
exceeded macro recursion depth. Splitting that object preserves every field and
criterion without changing global compiler limits or any audio code.

The original 72-stream measurement and the later 72-stream profile comparison
also pass again. All 144 raw/wet/dry hashes and stable stream coordinates match
the preceding delivery exactly. All 828 individual acoustic rows and the profile
aggregate match exactly, including the previous negative results. Dynamic owner
addresses and per-run source/map identity values are excluded from cross-run
equality; their real lifetime and currentness assertions still execute in each run.

Affected verification totals 13 distinct passing Native tests: four default
invariants/refusals, three explicitly executed measurements and six owning-birth
regressions. The final aligned rerun reproduces all eight acoustic rows exactly
after moving a constant horizon assertion into a const block. Release Clippy,
Cargo formatting, standalone formatting of included test files, affected strict
OpenSpec validation and diff checks pass. Clippy retains the existing warnings;
the new test warning is fixed. Production code and the installed native module
are unchanged, so the compatible preceding Release/import/preflight evidence is
reused. No App or audio device is started.

These eight cases establish bounded component evidence for two isolated synthetic
events, one pitch and two Native phases. They do not qualify tonal pitch, arbitrary
music, other shifts/rates, KEYLOCK, continuous changes, either complete start
policy, callback readiness/deadlines, ordinary Mixer adoption or effective SYNC.
Production profiles, public guards and scheduling remain unchanged. Earlier
negative Short/Medium and extreme-pitch results remain valid; no whole-program,
device, musical or Human acceptance follows from this result.
