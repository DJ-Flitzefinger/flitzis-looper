"""One joined control worker for output startup, Apply and failed-device retirement."""

import json
from dataclasses import dataclass
from threading import Thread
from typing import TYPE_CHECKING, Literal

from flitzis_looper.audio_output_options import AudioOutputOptions

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.controller.app import AppController
    from flitzis_looper.controller.audio_output_restore import OutputRollbackPlan

type OutputOperation = Literal[
    "startup", "apply", "rollback", "failure", "inventory", "commit", "abort"
]


@dataclass(slots=True)
class _Operation:
    kind: OutputOperation
    thread: Thread | None = None
    result: dict[str, object] | None = None
    error: str | None = None


class AudioOutputController:
    """Retain requested intent separately from initialized and observed geometry.

    Driver calls run on one transient worker. Its result is consumed only after
    an actual join; neither a receipt nor a dead callback replaces that barrier.
    Existing source and recording owners continue polling during failed-device
    close, whose Native API deliberately takes a shared Engine borrow.
    """

    def __init__(self, app: AppController) -> None:
        self._app = app
        self.draft = app.project.audio_output
        self.inventory: list[dict[str, object]] = []
        self.observed: dict[str, object] | None = None
        self.error: str | None = None
        self.previous_errors: dict[tuple[int, str], str] = {}
        self.phase = "unavailable"
        self.available = False
        self.recording_retired_before_worker = False
        self._operation: _Operation | None = None
        self._epoch: int | None = None
        self._previous: OutputRollbackPlan | None = None
        self._failed_close_verified = False
        self._failed_close_attempted = False
        self._apply_error: str | None = None
        self._commit_kind: OutputOperation | None = None
        self._persisted_options = app.project.audio_output
        self._persisted_recovery = app.persistence.audio_output_recovery
        self._empty_startup = False
        self._startup_draining = False
        self._source_startup_completed = False

    @property
    def busy(self) -> bool:
        """Whether an original worker or restore transaction still owns Apply."""
        return self._operation is not None or self.phase in {
            "restoring",
            "rolling_back",
            "cancelling",
        }

    @property
    def has_runtime(self) -> bool:
        """Whether original source owners have ever been activated in this App."""
        return self._app._runtime_activated

    @property
    def exclusive_native_access(self) -> bool:
        """Whether UI Native calls would conflict with the control worker borrow."""
        return self._operation is not None and self._operation.kind != "failure"

    @property
    def can_perform(self) -> bool:
        """Whether ordinary performer actions may admit new live work."""
        return self.phase == "ready" and self.available and not self.busy and self._epoch is None

    @property
    def transaction_active(self) -> bool:
        """Retain the original transaction even if restoration reports an error."""
        return self._epoch is not None

    def _submit(self, kind: OutputOperation, operation: Callable[[], dict[str, object]]) -> None:
        if self._operation is not None:
            message = "The original audio configuration worker has not joined"
            raise RuntimeError(message)
        self.recording_retired_before_worker = self._app.recording.status().retired
        owner = _Operation(kind)

        def run() -> None:
            try:
                owner.result = operation()
            except (OSError, RuntimeError, TypeError, ValueError) as error:
                owner.error = str(error)[:4096]

        owner.thread = Thread(target=run, name="audio-output-control", daemon=False)
        self._operation = owner
        try:
            owner.thread.start()
        except RuntimeError:
            self._operation = None
            raise

    def start_saved(self) -> None:
        """Attempt valid saved intent once; invalid saved fields require explicit Apply."""
        recovery = self._app.persistence.audio_output_recovery
        if recovery is not None:
            self.error = recovery.message
            self._app.session.settings_open = True
            return
        self.phase = "starting"
        self._submit("startup", lambda: self._open(self.draft))

    def _open(
        self, options: AudioOutputOptions, *, match_engine_format: bool | None = None
    ) -> dict[str, object]:
        if match_engine_format is None:
            self._app.audio.run_with_output_options(options.model_dump_json())
        else:
            self._app.audio.run_with_output_options(
                options.model_dump_json(), match_engine_format=match_engine_format
            )
        observed = self._app.audio.output_device_descriptor()
        if not isinstance(observed, dict):
            message = "Installed audio output has no configuration receipt"
            raise TypeError(message)
        return observed

    def refresh_inventory(self) -> None:
        """Discover names without opening an ASIO driver or a second audio stream."""
        if self.busy:
            return

        def discover() -> dict[str, object]:
            return {"inventory": json.loads(self._app.audio.audio_output_inventory_json())}

        self._submit("inventory", discover)

    def apply(self, options: AudioOutputOptions) -> bool:
        """Claim a fully stopped change before releasing the existing device."""
        if self.busy:
            return False
        self.draft = AudioOutputOptions.model_validate(options)
        try:
            self._require_compatible_pair_rate(self.draft)
            if self._epoch is None:
                self._previous = self._resolved_previous() if self.available else None
                self._epoch = self._app.audio_output_runtime.prepare_change()
                self._persisted_options = self._app.project.audio_output
                self._persisted_recovery = self._app.persistence.audio_output_recovery
            else:
                self._app.audio_output_runtime.require_change_idle(self._epoch)
            self.available = False
            self.error = None
            self._apply_error = None
            self._commit_kind = "apply"
            self._failed_close_verified = False
            self._failed_close_attempted = False
            self.phase = "applying"
            self._startup_draining = False
            self._empty_startup = False
            self._app.session.settings_open = True
            self._submit("apply", lambda: self._reopen(self.draft, explicit=True))
        except (OSError, RuntimeError, TypeError, ValueError) as error:
            self.error = str(error)
            return False
        return True

    def _require_compatible_pair_rate(self, options: AudioOutputOptions) -> None:
        """Preserve established exact-rate pairs before claiming the old output."""
        if not self._source_startup_completed or not any(
            path is not None and entry is not None and entry.pair is not None
            for path, entry in zip(
                self._app.project.sample_paths, self._app.project.stem_cache, strict=True
            )
        ):
            return
        rate = (
            self._previous.options.sample_rate_hz
            if self._epoch is not None and self._previous is not None
            else (self.observed or {}).get("sample_rate_hz")
        )
        if type(rate) is not int or rate <= 0:
            message = "The existing paired-stem output rate is unavailable"
            raise RuntimeError(message)
        if options.sample_rate_hz != rate:
            message = f"Assigned paired stems require a fixed output rate of {rate} Hz"
            raise RuntimeError(message)

    def _resolved_previous(self) -> OutputRollbackPlan:
        if self.observed is None:
            message = "Original output configuration is unavailable for rollback"
            raise RuntimeError(message)
        return self._app.audio_output_runtime.resolved_output(self.observed)

    def _reopen(
        self,
        options: AudioOutputOptions,
        *,
        explicit: bool = False,
        match_engine_format: bool | None = None,
    ) -> dict[str, object]:
        assert self._epoch is not None
        self._app.audio.prepare_audio_output_restore_plan(self._epoch, options.sample_rate_hz)
        receipt = self._app.audio.retire_audio_output_for_change(self._epoch)
        try:
            self._validate_rate_restoration(receipt)
        except RuntimeError as error:
            if not explicit:
                raise
            self._apply_error = f"Explicit Apply follows the reported prior-rate conflict: {error}"
        self._app.audio_output_runtime.retire_projections()
        self._app.audio.reset_after_audio_output_retirement(self._epoch)
        return self._open(options, match_engine_format=match_engine_format)

    @staticmethod
    def _validate_rate_restoration(receipt_json: str | None) -> None:
        if receipt_json is None:
            return
        receipt = json.loads(receipt_json)
        asio = receipt.get("asio_control", {})
        transaction = asio.get("rate_transaction", {})
        if transaction.get("disposition", 0) in {3, 4, 5}:
            message = (
                "The original driver was physically closed, but its prior global sample rate "
                "could not be restored safely. The driver reported a conflict, unknown rate "
                "or restoration failure. Review its current settings before another Apply."
            )
            raise RuntimeError(message)

    def request_failed_close(self) -> bool:
        """Schedule the original failed owner close and report only its joined result."""
        if self._failed_close_verified:
            return True
        if self._operation is None and not self._failed_close_attempted:
            self._failed_close_attempted = True
            self.available = False
            self.phase = "device_failed"

            def close_original() -> dict[str, object]:
                closed = self._app.audio.close_failed_audio_output_owner()
                return {
                    "closed": closed,
                    "close_receipt": self._app.audio.last_audio_output_close_receipt_json(),
                }

            self._submit("failure", close_original)
        return False

    def retry_failed_close(self) -> None:
        """Retry the same retained owner only after an explicit Settings gesture."""
        if self._operation is not None:
            return
        self._failed_close_attempted = False
        self.request_failed_close()

    def cancel(self) -> None:
        """Cancel pending restoration while keeping original admitted readers owned."""
        if self._operation is not None or self.phase == "cancelling":
            return
        if self._epoch is None:
            if self.phase in {"startup_restoring", "startup_failed"}:
                self._startup_draining = True
                self._app.audio_output_runtime.cancel_admissions()
                self.phase = "startup_cancelled"
                self.error = "Startup restoration cancelled; admitted readers are still settling"
            return
        self._apply_error = "Audio output change cancelled"
        self._app.audio_output_runtime.cancel_admissions()
        self.phase = "cancelling"

    def poll(self) -> None:
        """Collect the actual worker before advancing the original ordered phases."""
        if not self._collect_joined_operation():
            return
        if self.phase == "cancelling":
            self._poll_cancellation()
        elif self.phase == "startup_restoring":
            self._poll_startup_restore()
        elif self.phase in {"restoring", "rolling_back"}:
            self._poll_restore()
        elif (
            self.phase == "restore_failed"
            and self._epoch is not None
            and self._commit_kind == "apply"
        ):
            self._poll_failed_apply_restore()
        elif self.available and self._epoch is None:
            self._poll_installed_output()

    def _collect_joined_operation(self) -> bool:
        owner = self._operation
        if owner is None:
            return True
        assert owner.thread is not None
        if owner.thread.is_alive():
            return False
        owner.thread.join()
        self._operation = None
        self._complete(owner)
        return self._operation is None

    def _poll_cancellation(self) -> None:
        assert self._epoch is not None
        self._app.audio_output_runtime.cancel_admissions()
        try:
            self._app.audio_output_runtime.require_change_idle(self._epoch)
        except RuntimeError, TypeError, ValueError:
            return
        self._rollback(self._apply_error or "Audio output change cancelled")

    def _poll_startup_restore(self) -> None:
        if self._app.audio.audio_stream_failed():
            self._startup_draining = True
            self._app.audio_output_runtime.cancel_admissions()
            self.request_failed_close()
            return
        try:
            if self._app.audio_output_runtime.startup_restored():
                self._app.audio_output_runtime.finish_startup()
                self._source_startup_completed = True
                self.phase = "ready"
        except (OSError, RuntimeError, TypeError, ValueError) as error:
            self.phase = "startup_failed"
            self.error = str(error)

    def _poll_failed_apply_restore(self) -> None:
        assert self._epoch is not None
        try:
            self._app.audio_output_runtime.require_change_idle(self._epoch)
        except RuntimeError, TypeError, ValueError:
            return
        self._rollback(self.error or "Audio output restoration failed")

    def _poll_installed_output(self) -> None:
        if self._app.audio.audio_stream_failed():
            self.request_failed_close()
        else:
            self.observed = self._app.audio.output_device_descriptor()

    def _complete(self, owner: _Operation) -> None:
        if owner.error is not None or owner.result is None:
            self._complete_operation_error(owner)
        else:
            self._complete_operation_result(owner.kind, owner.result)

    def _complete_operation_error(self, owner: _Operation) -> None:
        self.error = owner.error or "Audio control worker ended without a completion receipt"
        if owner.kind in {"inventory", "failure"}:
            return
        if owner.kind == "apply" and self._epoch is not None:
            self._complete_apply_error()
            return
        if (
            owner.kind == "commit"
            and self._epoch is not None
            and self._handle_failed_commit_receipt()
        ):
            return
        if owner.kind == "apply" or (owner.kind == "commit" and self._commit_kind == "apply"):
            self._rollback(self.error)
        else:
            self.phase = "unavailable"
            self._app.session.settings_open = True

    def _complete_apply_error(self) -> None:
        assert self._epoch is not None
        cancelled: bool | None
        try:
            cancelled = self._app.audio.cancel_audio_output_change_before_close(self._epoch)
        except (OSError, RuntimeError, TypeError, ValueError) as error:
            cancelled = self._reconcile_preclose_cancel_error(error)
        if cancelled is None:
            return
        if cancelled:
            self._finish_preclose_cancel()
        else:
            self.phase = "restore_failed"

    def _reconcile_preclose_cancel_error(self, error: BaseException) -> bool | None:
        assert self._epoch is not None
        self.error = f"{self.error}; pre-close cancellation remains unverified: {error}"
        # A lost receipt proves neither cancellation nor an untouched latch.
        try:
            actual_epoch = self._app.audio.audio_output_change_epoch()
        except RuntimeError, TypeError, ValueError:
            self.phase = "unavailable"
            return None
        if actual_epoch != self._epoch + 1 or actual_epoch % 2:
            self.phase = "unavailable"
            return None
        return True

    def _finish_preclose_cancel(self) -> None:
        self._epoch = None
        self.available = not self._app.audio.audio_stream_failed()
        self.phase = "ready" if self.available else "device_failed"
        self._app.audio_output_runtime.release_completed_restore()

    def _handle_failed_commit_receipt(self) -> bool:
        assert self._epoch is not None
        try:
            actual_epoch = self._app.audio.audio_output_change_epoch()
        except (RuntimeError, TypeError, ValueError) as error:
            self.error = f"{self.error}; final Native epoch remains unverified: {error}"
            self.phase = "unavailable"
            return True
        if actual_epoch == self._epoch + 1 and actual_epoch % 2 == 0:
            # The actual final latch completed; never abort its successor epoch.
            self._finish_commit()
            return True
        return False

    def _complete_operation_result(self, kind: OutputOperation, result: dict[str, object]) -> None:
        if kind == "inventory":
            self._complete_inventory(result)
        elif kind == "failure":
            self._complete_failed_close(result)
        elif kind == "abort":
            self.phase = "unavailable"
            self.error = self._apply_error
        elif kind == "commit":
            self.observed = result
            self._finish_commit()
        else:
            self._complete_installed_output(kind, result)

    def _complete_inventory(self, result: dict[str, object]) -> None:
        inventory = result.get("inventory")
        if not isinstance(inventory, list) or not all(isinstance(item, dict) for item in inventory):
            self.error = "Output discovery returned an invalid inventory"
            return
        self.inventory = inventory

    @staticmethod
    def _optional_close_receipt(value: object) -> str | None:
        if value is not None and not isinstance(value, str):
            message = "Original failed-output close has an invalid receipt"
            raise TypeError(message)
        return value

    def _complete_failed_close(self, result: dict[str, object]) -> None:
        self._failed_close_verified = result.get("closed") is True
        if self._failed_close_verified and self._epoch is not None:
            try:
                self._validate_rate_restoration(
                    self._optional_close_receipt(result.get("close_receipt"))
                )
            except (RuntimeError, TypeError, ValueError) as error:
                self.error = str(error)
                self.phase = "unavailable"
            else:
                self.phase = "restore_failed"

    def _complete_installed_output(self, kind: OutputOperation, result: dict[str, object]) -> None:
        self.observed = result
        self.available = True
        # Only a newly installed owner creates a new close obligation.
        self._failed_close_verified = False
        self._failed_close_attempted = False
        try:
            if self._epoch is None:
                self._commit_kind = "startup"
                self._commit()
            elif not self._app._runtime_activated:
                # Whole source startup still follows this device-only epoch.
                self._empty_startup = True
                self._commit_kind = "apply"
                self._commit()
            else:
                self._commit_kind = kind
                self.phase = "rolling_back" if kind == "rollback" else "restoring"
                self._app.audio.enable_audio_output_restore(self._epoch)
                self._app.audio.with_audio_output_restore(
                    self._epoch, self._app.audio_output_runtime.restore_sources
                )
        except (OSError, RuntimeError, TypeError, ValueError) as error:
            self.error = str(error)
            self.phase = "restore_failed"

    def _finish_commit(self) -> None:
        self._epoch = None
        self.available = not self._app.audio.audio_stream_failed()
        self.phase = "ready" if self.available else "device_failed"
        self.error = self._apply_error
        self._app.audio_output_runtime.release_completed_restore()
        if not self.available:
            self.request_failed_close()
            return
        if self._commit_kind == "startup" or self._empty_startup:
            self.phase = "startup_restoring"
            try:
                self._app.audio_output_runtime.activate()
            except (OSError, RuntimeError, TypeError, ValueError) as error:
                self.error = str(error)
                self.phase = "startup_failed"
        elif self.available:
            # A corrective Apply can complete sources after partial startup.
            self._source_startup_completed = True

    def poll_restore_events(self, callback: Callable[[], None]) -> None:
        """Scope only model-free restore events to this original Native transaction."""
        if self._startup_draining:
            self._app.audio_output_runtime.cancel_admissions()
            self._app._poll_cancelled_startup_events()
            return
        if self._epoch is None:
            callback()
            return
        if self.available and self._app.audio.audio_stream_failed():
            self._app.audio_output_runtime.cancel_admissions()
            self.request_failed_close()
        if self.available:
            try:
                self._app.audio.with_audio_output_restore(
                    self._epoch, self._app._poll_audio_output_restore_events
                )
            except (OSError, RuntimeError, TypeError, ValueError) as error:
                self.error = str(error)
                self.phase = "restore_failed"
        else:
            self._app._poll_audio_output_restore_events()

    def _poll_restore(self) -> None:
        assert self._epoch is not None
        try:
            if not self._app.audio.with_audio_output_restore(
                self._epoch, self._app.audio_output_runtime.sources_restored
            ):
                return
            self._app.audio.with_audio_output_restore(
                self._epoch, self._app.audio_output_runtime.finish_restore
            )
            self._commit()
        except (OSError, RuntimeError, TypeError, ValueError) as error:
            self.error = str(error)
            self.phase = "restore_failed"

    def _commit(self) -> None:
        self.phase = "committing"

        def commit() -> dict[str, object]:
            if self._epoch is not None:
                if self._commit_kind == "apply":
                    self._app.persistence.commit_audio_output(self.draft)
                else:
                    self._app.persistence.restore_audio_output_intent(
                        self._persisted_options, self._persisted_recovery
                    )
            self._app.audio.commit_audio_output_configuration()
            observed = self._app.audio.output_device_descriptor()
            if not isinstance(observed, dict):
                message = "Committed audio output has no configuration receipt"
                raise TypeError(message)
            if self._epoch is not None:
                self._app.audio_output_runtime.finish_native_change(
                    self._epoch, empty_startup=self._empty_startup
                )
            return observed

        self._submit("commit", commit)

    def _rollback(self, reason: str) -> None:
        self._apply_error = reason
        self.available = False
        if self._previous is None:
            self.phase = "aborting"
            self._submit("abort", self._abort)
            return
        self.phase = "rolling_back"
        self._submit("rollback", self._restore_previous)

    def _restore_previous(self) -> dict[str, object]:
        assert self._previous is not None
        persistence_error = None
        self._app.audio_output_runtime.restore_original_grid()
        try:
            self._app.persistence.restore_audio_output_intent(
                self._persisted_options, self._persisted_recovery
            )
        except (OSError, RuntimeError, TypeError, ValueError) as error:
            persistence_error = str(error)
        # Disk compensation and physical rollback have independent outcomes.
        # A failed preference write must not suppress the actual close attempt.
        try:
            observed = self._reopen(
                self._previous.options, match_engine_format=self._previous.match_engine_format
            )
        except (OSError, RuntimeError, TypeError, ValueError) as error:
            message = f"Audio output rollback failed: {error}"
            if persistence_error is not None:
                message += f"; prior settings could not be saved: {persistence_error}"
            raise RuntimeError(message) from error
        if persistence_error is not None:
            observed["prior_settings_compensation_error"] = persistence_error
        return observed

    def _abort(self) -> dict[str, object]:
        assert self._epoch is not None
        receipt = self._app.audio.retire_audio_output_for_change(self._epoch)
        self._app.audio_output_runtime.retire_projections()
        self._app.audio.reset_after_audio_output_retirement(self._epoch)
        self._app.audio_output_runtime.restore_original_grid()
        self._app.persistence.restore_audio_output_intent(
            self._persisted_options, self._persisted_recovery
        )
        self._validate_rate_restoration(receipt)
        return {"closed": True}
