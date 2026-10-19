## 1. Reproduction and repair

- [x] Reproduce uncaught GUI grid/start failures using synthetic controls.
- [x] Reproduce adopted-window reconciliation across a later timing revision.
- [x] Repair source mirroring without granting timing or launch authority.
- [x] Contain GUI admission failures and abort dependent restarts.

## 2. Validation

- [x] Run native resident/stem/launch and negative ownership regressions.
- [x] Run impacted Python UI/loop/playback/residency tests and static checks.
- [x] Validate this OpenSpec change strictly and review the isolated final diff.
- [x] Build and verify a private Release artifact if native production changes are needed.

Validation: 1466 integrated Python tests pass. Final focused native run has 102 passes and
one separately reproduced pre-fix failure; final race/RT run has four passes, including
two overlapping race tests (104 distinct passing native tests across these runs).
The broader 135-test run was 90 pass/45 fail. Its 45 failures were repeated with the
old mirror predicate and with this fix: both runs 20 pass/25 fail, identical failure
names. These preexisting Key Lock/runtime failures remain open; no full-suite, device
or listening acceptance is claimed. One independent review was CLEAR. Release and
installed Debug imports plus nine existing API preflight families pass without devices.
