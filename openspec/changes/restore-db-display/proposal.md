# Restore dB Display in Volume +EQ Window

## Change ID
`restore-db-display`

## Why
Users require precise volume control for audio mixing, and the dB display provides essential visual feedback for accurate adjustments. Without this display, users must rely solely on the slider position, which makes fine-tuning difficult.

## What Changes
This change modifies the Volume +EQ dialog implementation in `flitzis_looper/ui/dialogs/volume.py` to restore the visibility of the dB display above the gain slider. The functionality already exists in the code but appears to have positioning or visibility issues that prevent it from being seen by users.

## Problem Statement
The dB display that was previously visible above the gain slider in the "Volume +EQ" window is no longer visible to users, despite the functionality still being implemented in the code. Users have reported that this display was helpful for precise volume adjustments.

## Proposed Solution
Investigate why the dB display is not visible and restore its functionality. The code shows that a dynamic dB label is implemented to follow the slider position, but it may have positioning or visibility issues.

## Impact
- Restores a useful visual feedback mechanism for audio level adjustment
- Improves user experience by providing precise dB value indication
- No functional changes to the underlying audio processing

## Dependencies
None

## Rollback Plan
Revert the changes to `flitzis_looper/ui/dialogs/volume.py` if issues arise.