## ADDED Requirements

### Requirement: Windows BS-RoFormer setup verifies derived model artifacts

Setup SHALL derive model artifacts from the byte-verified release checkpoint
and configuration, verify the worker and exact runtime dependencies, and
publish selection only after staged checks. The installed worker SHALL use
relative package paths and a checkout-appropriate installation location.

#### Scenario: A healthy installation is checked or moved

- **WHEN** the worker package and its relative assets match the selected identity
- **THEN** a resource check SHALL report the same selection without inference or
  dependency downloads.

#### Scenario: A staged install fails

- **WHEN** model derivation, package qualification or provisioning fails
- **THEN** setup SHALL preserve the previous usable installation and any held
  native binary.
