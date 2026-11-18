#!/usr/bin/env python3
"""View-boundary gate: one module owns the UI framework.

GATES (exit 1 on any):

  * a production file (`src/bin/gui`, tests excluded) that is not `view.rs`
    mentions `egui::`;
  * a production file outside {view.rs, main.rs, app.rs} mentions `eframe::`;
  * the door stopped being the door (`view.rs` no longer re-exports
    `eframe::egui`), or an entry point stopped being the entry.

Production code gets the framework's names from `view.rs` (unqualified, through
the crate prelude) and never spells the framework namespace itself. `eframe`
stays at the entry: the door re-exports the context, and `main.rs` / `app.rs`
own `run_native`, `impl eframe::App` and the storage hooks. A framework change
starts in the door.

Run: py -3 scripts/check_egui_surface.py
"""

from __future__ import annotations

import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
GUI = REPO / "src" / "bin" / "gui"

EGUI = "egui::"
EFRAME = "eframe::"
DOOR = "view.rs"
ENTRY = {"main.rs", "app.rs"}


def production_sources() -> list[Path]:
    return sorted(
        path
        for path in GUI.rglob("*.rs")
        if "tests" not in path.relative_to(GUI).parts and path.name != "tests.rs"
    )


def main() -> int:
    counts: dict[str, tuple[int, int]] = {}
    for path in production_sources():
        rel = path.relative_to(GUI).as_posix()
        src = path.read_text(encoding="utf-8")
        counts[rel] = (src.count(EGUI), src.count(EFRAME))

    failures: list[str] = []

    for rel, (egui, _) in sorted(counts.items()):
        if rel != DOOR and egui:
            failures.append(
                f"{rel}: names the framework namespace (`{EGUI}`) — import the "
                "name from `view.rs` instead"
            )

    for rel, (_, eframe) in sorted(counts.items()):
        if rel not in {"view.rs", "main.rs", "app.rs"} and eframe:
            failures.append(
                f"{rel}: reaches for `{EFRAME}` — only the door and the two "
                "entry points may name the framework shell"
            )

    door = (GUI / DOOR).read_text(encoding="utf-8") if (GUI / DOOR).is_file() else ""
    if "eframe::egui" not in door:
        failures.append(f"{DOOR}: no longer re-exports `eframe::egui` — the door is gone")

    for entry in sorted(ENTRY):
        if counts.get(entry, (0, 0))[1] == 0:
            failures.append(f"{entry}: no `{EFRAME}` left — the entry stopped being the entry")

    for failure in failures:
        print(f"FAIL {failure}")

    egui_outside = sum(n for rel, (n, _) in counts.items() if rel != DOOR)
    eframe_files = [rel for rel, (_, n) in counts.items() if n]
    verdict = "PASS" if not failures else "FAIL"
    print(
        f"check_view_boundary: {verdict} — {egui_outside} `{EGUI}` outside the door; "
        f"`{EFRAME}` in {', '.join(sorted(eframe_files))}"
    )
    return 0 if not failures else 1


if __name__ == "__main__":
    sys.exit(main())
