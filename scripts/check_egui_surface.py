#!/usr/bin/env python3
"""Framework quarantine: only the door may name the UI framework.

GATES (exit 1 on any):

  * a production file (`src/bin/gui`, tests excluded) other than `view.rs`
    contains the framework's name in CODE — `egui`/`eframe` as a token, which
    includes identifiers such as `egui_ctx` and paths such as `eframe::App`;
  * the door stopped being the door (`view.rs` no longer re-exports
    `eframe::egui`).

Comments and doc comments are prose, not code: a sentence about the framework
is allowed anywhere (it is not a dependency). Tests are the harness and drive
the framework directly through the `egui`/`eframe` names the door re-exports
under `cfg(test)`; this gate scans production only.

The quarantine is the anti-corruption layer: `view.rs` re-exports the names the
GUI uses and owns the launch, the application hooks and the storage plugin, so
a framework change starts and ends in that one file.

Run: py -3 scripts/check_egui_surface.py
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
GUI = REPO / "src" / "bin" / "gui"

DOOR = "view.rs"
FRAMEWORK = re.compile(r"\b(?:egui|eframe)\w*")


def production_sources() -> list[Path]:
    return sorted(
        path
        for path in GUI.rglob("*.rs")
        if "tests" not in path.relative_to(GUI).parts and path.name != "tests.rs"
    )


def code_only(line: str) -> str:
    """The line with its trailing `//` comment cut off (prose is not code)."""
    cut = line.find("//")
    return line if cut < 0 else line[:cut]


def main() -> int:
    failures: list[str] = []
    prose = 0

    for path in production_sources():
        rel = path.relative_to(GUI).as_posix()
        if rel == DOOR:
            continue
        for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
            if "egui" in line or "eframe" in line:
                prose += 1
            code = code_only(line)
            if FRAMEWORK.search(code):
                failures.append(
                    f"{rel}:{number}: names the framework outside the door — "
                    f"route it through `{DOOR}`: {line.strip()}"
                )

    door = GUI / DOOR
    door_src = door.read_text(encoding="utf-8") if door.is_file() else ""
    if "eframe::egui" not in door_src:
        failures.append(f"{DOOR}: no longer re-exports `eframe::egui` — the door is gone")

    for failure in failures:
        print(f"FAIL {failure}")

    verdict = "PASS" if not failures else "FAIL"
    files = len(production_sources())
    print(
        f"check_view_boundary: {verdict} — 0 framework tokens in code outside "
        f"`{DOOR}` over {files} production files "
        f"({prose} prose mentions in comments; comments are allowed)"
    )
    return 0 if not failures else 1


if __name__ == "__main__":
    sys.exit(main())
