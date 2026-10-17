#!/usr/bin/env python3
"""Regenerate vino_speak/model_pins.json for a Hugging Face revision.

Deliberately a script and not automatic: the diff is a list of hashes, and it
is meant to be read in review.

    python scripts/update_pins.py                # the repository's current head
    python scripts/update_pins.py REVISION       # a specific commit

Downloads the whole revision (~160 MB) to a scratch directory under
~/.cache, hashes every file, and cross-checks the LFS files against the sha256
the Hub API reports for them.
"""

from __future__ import annotations

import json
import shutil
import sys
from datetime import date
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))

from huggingface_hub import HfApi, snapshot_download  # noqa: E402

from vino_speak.model import MODEL_ID, PINS_PATH, sha256_file  # noqa: E402

# Never used by the pipeline.
SKIP = {".gitattributes", "README.md"}


def main(argv: list[str]) -> int:
    api = HfApi()
    revision = argv[0] if argv else api.model_info(MODEL_ID).sha
    if not revision:
        raise SystemExit(f"The Hub reported no revision for {MODEL_ID}.")
    infos = {
        entry.path: entry
        for entry in api.list_repo_tree(MODEL_ID, revision=revision, recursive=True)
        if hasattr(entry, "size") and entry.path not in SKIP
    }

    scratch = Path.home() / ".cache" / "vinospeak" / "pin-scratch"
    shutil.rmtree(scratch, ignore_errors=True)
    snapshot_download(
        repo_id=MODEL_ID,
        revision=revision,
        local_dir=str(scratch),
        allow_patterns=list(infos),
    )

    files = {}
    for name in sorted(infos):
        path = scratch / name
        digest = sha256_file(path)
        hub = getattr(getattr(infos[name], "lfs", None), "sha256", None)
        if hub and hub != digest:
            raise SystemExit(f"{name}: downloaded sha256 {digest} != Hub LFS sha256 {hub}")
        if path.stat().st_size != infos[name].size:
            raise SystemExit(f"{name}: size differs from the Hub listing")
        files[name] = {"sha256": digest, "size": path.stat().st_size}
    shutil.rmtree(scratch)

    record = {
        "schema": 1,
        "generated_by": "scripts/update_pins.py, generated data, do not edit by hand",
        "model_id": MODEL_ID,
        "revision": revision,
        "recorded": date.today().isoformat(),
        "files": files,
    }
    PINS_PATH.write_text(json.dumps(record, indent=2) + "\n", encoding="utf-8")
    print(f"pinned {len(files)} files at {revision}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
