"""Control-owned monotone stem intent; full producer verification remains off-thread."""

import re
from dataclasses import dataclass, replace
from typing import TYPE_CHECKING

from flitzis_looper.controller.stem_identity import request_signature, source_digest

if TYPE_CHECKING:
    from collections.abc import Iterable

    from flitzis_looper.controller.stem_generation import StemGenerationRequest


@dataclass(frozen=True, slots=True)
class StemGenerationIntent:
    """One captured admission, independent of mutable Settings and producer resolution."""

    settings_revision: int
    material_revision: int
    admission_sequence: int
    material_key: str
    signature: tuple[object, ...]


@dataclass(slots=True)
class _MaterialIntent:
    settings_revision: int
    signature: tuple[object, ...]
    material_revision: int
    context_revision: int
    resolved_key: str | None = None
    resolved_admission_sequence: int = 0


@dataclass(slots=True)
class _Admission:
    captured: StemGenerationIntent
    bound: StemGenerationIntent | None = None
    resolved_key: str | None = None


class StemIntentRegistry:
    """Linearize admissions and resolved producer replacement on the control owner.

    Equal configuration/Settings admissions share a material revision. Resolution
    can advance that revision for newer actual producer bytes. Earlier or already
    obsolete resolved admissions cannot restore a previous producer, including ABA.
    """

    def __init__(self) -> None:
        self._materials: dict[str, _MaterialIntent] = {}
        self._admissions: dict[int, _Admission] = {}
        self._admission_sequence = 0
        self._material_revision = 0
        self._settings_revision = 0

    def capture(
        self, request: StemGenerationRequest, settings_revision: int
    ) -> StemGenerationIntent:
        """Capture one full-source admission without model hashing or native rights."""
        if type(settings_revision) is not int or settings_revision < self._settings_revision:
            message = "stem Settings revision must be a current nonnegative integer"
            raise ValueError(message)
        material_key = source_digest(request.source_version)
        if material_key is None:
            message = "stem intent requires a verified complete source digest"
            raise ValueError(message)
        self._settings_revision = settings_revision
        signature = request_signature(request)
        current = self._materials.get(material_key)
        if current is None or (
            current.settings_revision != settings_revision or current.signature != signature
        ):
            self._material_revision += 1
            current = _MaterialIntent(
                settings_revision,
                signature,
                self._material_revision,
                self._material_revision,
            )
            self._materials[material_key] = current
        self._admission_sequence += 1
        intent = StemGenerationIntent(
            settings_revision,
            current.material_revision,
            self._admission_sequence,
            material_key,
            signature,
        )
        self._admissions[intent.admission_sequence] = _Admission(intent)
        return intent

    def bind(self, intent: StemGenerationIntent, resolved_key: str) -> StemGenerationIntent | None:
        """Bind verified bytes once; return the effective revision or reject obsolete work.

        A newer unbound admission may have been captured before an intermediate
        producer resolution. Bind it before testing currentness, then store the
        returned frozen intent. Equal keys retain compatible current subscribers.
        """
        if re.fullmatch(r"[0-9a-f]{64}", resolved_key) is None:
            message = "resolved stem identity must be a complete fingerprint"
            raise ValueError(message)
        admission = self._admissions.get(intent.admission_sequence)
        if admission is None or intent not in {admission.captured, admission.bound}:
            return None
        if admission.resolved_key is not None:
            return self._bound_result(admission, resolved_key)
        # One admission resolves actual producer custody once. A rejected result
        # cannot later retry with different bytes under the same request identity.
        admission.resolved_key = resolved_key
        current = self._materials.get(intent.material_key)
        if current is None or not self._same_context(intent, current):
            return None
        return self._bind_unresolved(intent, admission, current, resolved_key)

    def _bound_result(
        self, admission: _Admission, resolved_key: str
    ) -> StemGenerationIntent | None:
        if (
            admission.resolved_key != resolved_key
            or admission.bound is None
            or not self.is_current(admission.bound)
        ):
            return None
        return admission.bound

    def _bind_unresolved(
        self,
        intent: StemGenerationIntent,
        admission: _Admission,
        current: _MaterialIntent,
        resolved_key: str,
    ) -> StemGenerationIntent | None:
        older = intent.admission_sequence <= current.resolved_admission_sequence
        if current.resolved_key is not None and current.resolved_key != resolved_key:
            if older:
                return None
            self._material_revision += 1
            current.material_revision = self._material_revision
        elif intent.material_revision != current.material_revision and older:
            return None
        current.resolved_key = resolved_key
        current.resolved_admission_sequence = max(
            current.resolved_admission_sequence, intent.admission_sequence
        )
        bound = replace(intent, material_revision=current.material_revision)
        admission.bound = bound
        admission.resolved_key = resolved_key
        return bound

    def is_current(self, intent: StemGenerationIntent) -> bool:
        """Check the registered monotone revision, never value equality alone."""
        admission = self._admissions.get(intent.admission_sequence)
        if admission is None or intent not in {admission.captured, admission.bound}:
            return False
        current = self._materials.get(intent.material_key)
        if current is None or not self._same_context(intent, current):
            return False
        if intent.material_revision != current.material_revision:
            return False
        if admission.bound is None:
            return admission.resolved_key is None
        return admission.bound == intent and admission.resolved_key == current.resolved_key

    def release(self, intent: StemGenerationIntent) -> None:
        """Forget a settled admission while retaining its material's monotone selection."""
        admission = self._admissions.get(intent.admission_sequence)
        if admission is not None and intent in {admission.captured, admission.bound}:
            self._admissions.pop(intent.admission_sequence)

    def prune(self, live_intents: Iterable[StemGenerationIntent]) -> None:
        """Retain only registered live interests without reusing any revision or admission."""
        live_sequences: set[int] = set()
        for intent in live_intents:
            admission = self._admissions.get(intent.admission_sequence)
            if admission is not None and intent in {admission.captured, admission.bound}:
                live_sequences.add(intent.admission_sequence)
        self._admissions = {
            sequence: item
            for sequence, item in self._admissions.items()
            if sequence in live_sequences
        }
        material_keys = {item.captured.material_key for item in self._admissions.values()}
        self._materials = {
            key: current for key, current in self._materials.items() if key in material_keys
        }

    def forget_material(self, material_key: str) -> None:
        """Drop unused discovery state without resetting the global revision sequence."""
        if any(item.captured.material_key == material_key for item in self._admissions.values()):
            message = "stem material still has admitted interests"
            raise RuntimeError(message)
        self._materials.pop(material_key, None)

    def _same_context(self, intent: StemGenerationIntent, current: _MaterialIntent) -> bool:
        return (
            intent.settings_revision == self._settings_revision
            and intent.settings_revision == current.settings_revision
            and intent.signature == current.signature
            and intent.material_revision >= current.context_revision
        )
