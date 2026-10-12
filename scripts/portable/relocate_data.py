"""Invoke the Native physical-custody transaction in the candidate private runtime."""

import argparse
import importlib
import json
import sys
from pathlib import Path

_CHECKOUT = Path(__file__).resolve().parents[2]


def main() -> int:
    """Relocate or recover Samples while retaining actual Native file identities."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--target", type=Path, required=True)
    parser.add_argument("--record", type=Path, required=True)
    parser.add_argument(
        "--action", choices=("capture", "move", "recover", "rollback"), required=True
    )
    arguments = parser.parse_args()
    runtime = Path(sys.executable).resolve().parents[1]
    sys.path.insert(0, str(_CHECKOUT))
    bootstrap = importlib.import_module("scripts.portable.bootstrap")
    handles = bootstrap.configure(arguments.root, _CHECKOUT, runtime)
    try:
        # Bootstrap must precede Native import; no Engine, GUI or audio device opens.
        native = importlib.import_module("flitzis_looper_audio")

        result = native.portable_relocate_data(
            str(arguments.root.absolute()),
            str(arguments.source.absolute()),
            str(arguments.target.absolute()),
            str(arguments.record.absolute()),
            arguments.action,
        )
        sys.stdout.write(result + "\n")
    except (OSError, RuntimeError, ValueError) as error:
        sys.stderr.write(json.dumps({"schema_version": 1, "error": str(error)}) + "\n")
        return 1
    finally:
        for handle in handles:
            handle.close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
