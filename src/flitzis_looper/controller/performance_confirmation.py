"""One transient admission owner for deliberate performer and Bank actions."""

from dataclasses import dataclass, field, replace
from typing import TYPE_CHECKING, Literal

from flitzis_looper.bank_snapshot import validate_current_bank_snapshot
from flitzis_looper.controller.key_metadata import loaded_source_identity
from flitzis_looper.models import validate_sample_id

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController
    from flitzis_looper.controller.bank_arrangement import BankRequest
    from flitzis_looper.controller.stem_batch_inventory import StemBatchInventory
    from flitzis_looper.controller.stem_delete_inventory import StemDeleteInventory

type ConfirmationAction = Literal["unload", "analyze"]
type BankConfirmationAction = Literal["copy_bank", "clear_bank"]


@dataclass(frozen=True)
class PerformanceConfirmation:
    """One captured content assignment; it grants no Native audio authority."""

    action: ConfirmationAction
    pad_id: int
    content_instance: str | None
    path: str
    source_identity: tuple[int, str, int, int]
    error: str | None = None


@dataclass(frozen=True)
class BankConfirmation:
    """The original complete Bank request, including its opaque Native witnesses."""

    action: BankConfirmationAction
    request: BankRequest
    error: str | None = None
    source_bank: int | None = field(init=False)
    target_bank: int = field(init=False)
    _target_occupied: bool = field(init=False, repr=False)

    def __post_init__(self) -> None:
        """Derive immutable display scalars once; the original request remains authority."""
        captured = self.request.bank.projected()
        object.__setattr__(self, "source_bank", captured.source_bank)
        object.__setattr__(self, "target_bank", captured.target_bank)
        object.__setattr__(
            self,
            "_target_occupied",
            any(row.before.source_identity is not None for row in captured.slots),
        )


@dataclass(frozen=True)
class StemBatchConfirmation:
    """Exact no-I/O all-slot inventory; individual stale targets settle as skips."""

    inventory: StemBatchInventory
    error: str | None = None


@dataclass(frozen=True)
class StemDeleteConfirmation:
    """Exact original loaded assignments and known sets, with no cleanup permission."""

    inventory: StemDeleteInventory
    error: str | None = None


type PendingConfirmation = (
    PerformanceConfirmation | BankConfirmation | StemBatchConfirmation | StemDeleteConfirmation
)


class PerformanceConfirmationController:
    """Keep performer warnings separate from loader and Residency transactions."""

    def __init__(self, app: AppController) -> None:
        self._app = app
        self._pending: PendingConfirmation | None = None

    @property
    def pending(self) -> PendingConfirmation | None:
        """Return the current immutable warning target, independently of selection."""
        return self._pending

    def request(self, action: ConfirmationAction, pad_id: int) -> None:
        """Capture eligible current content without executing or replacing a warning."""
        validate_sample_id(pad_id)
        if self._pending is not None or not self._eligible(action, pad_id):
            return
        path = self._app.project.sample_paths[pad_id]
        identity = self._source_identity(pad_id)
        if path is None or identity is None:
            return
        content = self._app.project.pad_content[pad_id]
        self._pending = PerformanceConfirmation(
            action, pad_id, content.instance_id if content is not None else None, path, identity
        )

    def request_bank_copy(self, source_bank: int, target_bank: int) -> None:
        """Copy an empty target immediately; retain occupied targets for overwrite consent."""
        self._request_bank(source_bank, target_bank)

    def request_bank_clear(self, target_bank: int) -> None:
        """Always request consent for this exact bank, including empty/current banks."""
        self._request_bank(None, target_bank)

    def request_all_loaded_stems(self) -> None:
        """Capture one all-bank warning without admitting any original reader."""
        if self._pending is not None or not self._batch_eligible():
            return
        self._pending = StemBatchConfirmation(self._app.stems.capture_generation_inventory())

    def _batch_eligible(self) -> bool:
        return not (
            self._app._close_requested
            or self._app.stem_batch.active
            or self._app.stem_delete.active
            or self._app.banks.active
            or self._app.arrangement.active
            or self._app.pairs.active
        )

    def request_all_loaded_stem_deletion(self) -> None:
        """Capture original deletion scope without withdrawing any pending work."""
        if self._pending is not None or not self._delete_eligible():
            return
        self._app.diagnostics.begin_attempt("stem_delete")
        try:
            inventory = self._app.stems.capture_deletion_inventory()
        except (RuntimeError, TypeError, ValueError) as error:
            self._app.diagnostics.observe("stem_delete", str(error))
            return
        self._pending = StemDeleteConfirmation(inventory)

    def _delete_eligible(self) -> bool:
        return not (
            self._app._close_requested
            or self._app.stem_delete.active
            or self._app.banks.active
            or self._app.arrangement.active
            or self._app.pairs.active
        )

    def _request_bank(self, source_bank: int | None, target_bank: int) -> None:
        if self._pending is not None:
            return
        self._app.diagnostics.begin_attempt("bank_arrangement")
        try:
            request = self._app.banks.capture_request(source_bank, target_bank)
        except (OSError, RuntimeError, TypeError, ValueError) as error:
            self._app.diagnostics.observe("bank_arrangement", str(error))
            return
        action: BankConfirmationAction = "clear_bank" if source_bank is None else "copy_bank"
        intent = BankConfirmation(action, request)
        self._pending = intent
        if source_bank is not None and not intent._target_occupied:
            self.accept(intent)

    def is_current(self, intent: PendingConfirmation) -> bool:
        """Validate the captured musical target without granting Native admission authority."""
        if self._pending is not intent:
            return False
        if isinstance(intent, BankConfirmation):
            return intent.error is None and self._bank_current(intent)
        if isinstance(intent, StemBatchConfirmation):
            return intent.error is None and self._batch_eligible()
        if isinstance(intent, StemDeleteConfirmation):
            return intent.error is None and self._delete_eligible()
        content = self._app.project.pad_content[intent.pad_id]
        return (
            intent.error is None
            and self._eligible(intent.action, intent.pad_id)
            and self._app.project.sample_paths[intent.pad_id] == intent.path
            and (content.instance_id if content is not None else None) == intent.content_instance
            and self._source_identity(intent.pad_id) == intent.source_identity
        )

    def _bank_current(self, intent: BankConfirmation) -> bool:
        if (
            self._app._close_requested
            or self._app.banks.active
            or self._app.arrangement.active
            or self._app.pairs.active
        ):
            return False
        try:
            validate_current_bank_snapshot(self._app.project, intent.request.bank)
        except TypeError, ValueError:
            return False
        # This is only a musical projection. The original opaque Native witnesses
        # stay in request; Bank.begin alone decides whether they remain admissible.
        return True

    def dismiss(self, intent: PendingConfirmation) -> None:
        """Discard this exact warning/request without cancelling any live transaction."""
        if self._pending is intent:
            if isinstance(intent, BankConfirmation):
                self._app.banks.discard_request(intent.request)
            self._pending = None

    def accept(self, intent: PendingConfirmation) -> None:
        """Consume once, then dispatch only the captured authoritative operation."""
        if self._pending is not intent:
            return
        valid = self.is_current(intent)
        self._pending = None
        if not valid:
            if isinstance(intent, BankConfirmation):
                self._app.banks.discard_request(intent.request)
            return
        if isinstance(intent, BankConfirmation):
            self._accept_bank(intent)
            return
        if isinstance(intent, StemBatchConfirmation):
            self._app.stem_batch.start(intent.inventory)
            return
        if isinstance(intent, StemDeleteConfirmation):
            self._app.stem_delete.start(intent.inventory)
            return
        operation = "analysis" if intent.action == "analyze" else intent.action
        self._app.diagnostics.begin_attempt(operation, intent.pad_id)
        try:
            if intent.action == "unload":
                self._app.loader.unload_sample(intent.pad_id)
            else:
                self._app.loader.analyze_sample_async(intent.pad_id)
        except (RuntimeError, ValueError) as error:
            # A consumed admission failure remains visible but cannot be dispatched again.
            # In particular, Native unload rejection occurs before any source/UI cleanup.
            self._pending = replace(intent, error=str(error))
            self._app.diagnostics.observe(operation, str(error), pad_id=intent.pad_id)

    def _accept_bank(self, intent: BankConfirmation) -> None:
        self._app.diagnostics.begin_attempt("bank_arrangement")
        try:
            accepted = self._app.banks.begin(intent.request)
        except (OSError, RuntimeError, TypeError, ValueError) as error:
            reason = str(error)
        else:
            if accepted:
                self._app.diagnostics.observe("bank_arrangement", self._app.banks.error)
                return
            reason = self._app.banks.error or "Bank request is no longer available for admission"
        self._app.banks.discard_request(intent.request)
        self._pending = replace(intent, error=reason)
        self._app.diagnostics.observe("bank_arrangement", reason)

    def _eligible(self, action: ConfirmationAction, pad_id: int) -> bool:
        if not self._app.loader.is_sample_loaded(pad_id):
            return False
        if self._app.loader.is_sample_loading(pad_id):
            return False
        return action != "analyze" or pad_id not in self._app.session.analyzing_sample_ids

    def _source_identity(self, pad_id: int) -> tuple[int, str, int, int] | None:
        try:
            return loaded_source_identity(self._app._audio, pad_id)
        except RuntimeError, ValueError:
            # A disappeared Native source grants no warning acceptance authority.
            return None
