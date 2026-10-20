"""Real startup coordinators and disk receipts around labelled scripted Native owners.

These tests cover Python coordinator lifetime and retry ordering. The existing
three-process Bank last-reader proof separately verifies actual Native/FS custody.
Only fake audit lists and local test aliases are cleared after terminal success;
the production startup/App remain alive throughout the collectibility assertion.
"""

import gc
import hashlib
import weakref
from typing import TYPE_CHECKING, cast

from flitzis_looper.controller.bank_arrangement_journal import (
    BankArrangementJournal,
    BankJournalOwner,
)
from flitzis_looper.controller.copy_arrangement_journal import (
    CopyArrangementJournal,
    CopyJournalOwner,
)
from flitzis_looper.controller.pair_arrangement_journal import (
    PairArrangementJournal,
    PairJournalOwner,
)
from tests.flitzis_looper.controller import (
    test_bank_arrangement_startup as bank_case,
)
from tests.flitzis_looper.controller import (
    test_copy_arrangement_startup as copy_case,
)
from tests.flitzis_looper.controller import (
    test_pair_arrangement_startup as pair_case,
)

if TYPE_CHECKING:
    from pathlib import Path

    import pytest

    from flitzis_looper_audio import RecoveredBankAcceptance

type _Journal = BankArrangementJournal | CopyArrangementJournal | PairArrangementJournal


def _settled_bytes(config: Path, revision: int, journal: _Journal) -> bytes:
    assert journal.phase == "settled"
    assert journal.committed_revision == revision
    written = config.read_bytes()
    assert journal.committed_config_sha256 == hashlib.sha256(written).hexdigest()
    assert journal.project_image_json.encode("utf-8") == written
    return written


def test_copy_terminal_custody_survives_reader_failure_then_releases_with_app_alive(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    config, prior = copy_case._prior(tmp_path, monkeypatch, recorded=False)
    app = copy_case._app(config)
    app._audio.ready = True
    startup = copy_case._startup(app)
    app._assets.readers[-1].release_pressure = 1

    startup.poll()
    acceptance = app._audio.captures[0]
    reference = weakref.ref(acceptance)
    assert startup.active
    assert startup.error == "journal asset reader release temporarily unavailable"
    assert startup._after is not None
    assert startup._after.acceptance is acceptance.native_view
    assert acceptance.released
    assert app.persistence._migration_owner == prior.transaction_id
    assert startup._custody.readers
    assert app._assets.retirement.closed == 0
    journal = CopyJournalOwner.open(config.parent, prior.transaction_id).journal
    assert journal is not None
    written = _settled_bytes(config, app.persistence.revision, journal)
    gc.collect()
    assert reference() is acceptance

    startup.poll()
    assert not startup.active
    assert startup.status == "completed"
    assert startup._app is app
    assert app.persistence._migration_owner is None
    assert all(reader.released for reader in app._assets.readers)
    assert app._audio.captures == [acceptance]
    assert app._audio.releases == [acceptance]
    assert config.read_bytes() == written
    app._audio.captures.clear()
    app._audio.validations.clear()
    app._audio.releases.clear()
    del acceptance
    gc.collect()
    assert reference() is None, "completed Copy startup still owns verification custody"
    assert startup._app is app
    assert not startup.active


def test_pair_terminal_custody_survives_writer_failure_then_releases_with_app_alive(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    config, prior = pair_case._prior(tmp_path, monkeypatch, after=True, recorded=False)
    app = pair_case._app(config)
    app._audio.ready = True
    startup = pair_case._startup(app)
    release = app.persistence.release_migration
    release_attempts: list[str] = []

    def release_once(owner: str) -> None:
        release_attempts.append(owner)
        if len(release_attempts) == 1:
            assert app.persistence._migration_owner == owner
            message = "scripted terminal writer release pressure"
            raise OSError(message)
        release(owner)

    monkeypatch.setattr(app.persistence, "release_migration", release_once)
    startup.poll()
    acceptance = app._audio.captures[0][2]
    reference = weakref.ref(acceptance)
    assert startup.active
    assert startup.error == "scripted terminal writer release pressure"
    assert startup._after is not None
    assert startup._after.acceptance is acceptance
    assert acceptance.released
    assert app.persistence._migration_owner == prior.transaction_id
    assert app._assets.retirement.closed == 0
    journal = PairJournalOwner.open(config.parent, prior.transaction_id).journal
    assert journal is not None
    written = _settled_bytes(config, app.persistence.revision, journal)
    gc.collect()
    assert reference() is acceptance

    startup.poll()
    assert not startup.active
    assert startup.status == "completed"
    assert startup._app is app
    assert app.persistence._migration_owner is None
    assert release_attempts == [prior.transaction_id, prior.transaction_id]
    assert len(app._audio.captures) == 1
    assert app._audio.captures[0][2] is acceptance
    assert app._audio.releases == [acceptance]
    assert config.read_bytes() == written
    app._audio.captures.clear()
    app._audio.validations.clear()
    app._audio.releases.clear()
    del acceptance
    gc.collect()
    assert reference() is None, "completed Pair startup still owns verification custody"
    assert startup._app is app
    assert not startup.active


def test_bank_terminal_custody_survives_reader_failure_then_releases_with_app_alive(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    config, prior = bank_case._prior(tmp_path, monkeypatch, recorded=False)
    app = bank_case._app(config)
    startup = bank_case._startup(app)
    app._assets.readers[-1].release_pressure = 1
    startup.poll()
    acceptance = app._audio.captures[0]
    reference = weakref.ref(acceptance)
    # Only the existing scripted Native facade is advanced here. This is not a
    # callback ACK or filesystem lifetime proof.
    acceptance.status = "accepted"
    startup.poll()
    assert startup.active
    assert startup.error == "journal asset reader release temporarily unavailable"
    assert startup._after is not None
    assert startup._after.acceptance is cast("RecoveredBankAcceptance", acceptance)
    assert acceptance.status == "released"
    assert app.persistence._migration_owner == prior.transaction_id
    assert startup._custody.readers
    assert app._assets.retirement.closed == 0
    journal = BankJournalOwner.open(config.parent, prior.transaction_id).journal
    assert journal is not None
    written = _settled_bytes(config, app.persistence.revision, journal)
    gc.collect()
    assert reference() is acceptance

    startup.poll()
    assert not startup.active
    assert startup.status == "completed"
    assert startup._app is app
    assert app.persistence._migration_owner is None
    assert all(reader.released for reader in app._assets.readers)
    assert app._audio.captures == [acceptance]
    assert app._audio.releases == [acceptance]
    assert config.read_bytes() == written
    app._audio.captures.clear()
    app._audio.validations.clear()
    app._audio.retries.clear()
    app._audio.releases.clear()
    del acceptance
    gc.collect()
    assert reference() is None, "completed Bank startup still owns verification custody"
    assert startup._app is app
    assert not startup.active
