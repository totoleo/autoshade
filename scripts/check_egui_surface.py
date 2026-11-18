#!/usr/bin/env python3
"""egui-surface gate: the framework boundary only shrinks.

GATES (exit 1 on any):

  * a production file (`src/bin/gui`, tests excluded) that is not on the
    allowlist mentions `egui::`;
  * the total `egui::` occurrence count exceeds BUDGET;
  * an allowlisted file no longer mentions `egui::` (the list must shrink with
    each migration slice, not rot).

The allowlist and the budget are both written down here on purpose: every
slice of the egui-surface work updates them, so the surface cannot grow back
between slices. Adding a framework mention to a new file means either routing
it through `ui_kit.rs` or extending the allowlist deliberately.

Run: py -3 scripts/check_egui_surface.py
"""

from __future__ import annotations

import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
GUI = REPO / "src" / "bin" / "gui"

TOKEN = "egui::"

# Production files permitted to mention egui. One entry disappears when a
# slice moves the file's last mention behind the vocabulary.
ALLOWED = frozenset(
    {
        "ai_start_actions.rs",
        "app.rs",
        "buttons.rs",
        "canvas.rs",
        "canvas_masters.rs",
        "canvas_plates.rs",
        "controls.rs",
        "editor.rs",
        "headings.rs",
        "history_actions.rs",
        "i18n.rs",
        "landing_canvas.rs",
        "landing_worker.rs",
        "main.rs",
        "model.rs",
        "panel_view.rs",
        "panels/ai.rs",
        "panels/develop.rs",
        "panels/develop_forms.rs",
        "panels/develop_widgets.rs",
        "panels/gallery.rs",
        "panels/grade_wheel.rs",
        "panels/histogram.rs",
        "panels/normal.rs",
        "panels/portrait.rs",
        "panels/recipes.rs",
        "panels/retouch.rs",
        "panels/settings.rs",
        "quit.rs",
        "sections.rs",
        "services.rs",
        "shortcuts.rs",
        "side_panel.rs",
        "sliders.rs",
        "theme.rs",
        "ui_kit.rs",
        "util.rs",
        "variant_actions.rs",
        "window_chrome.rs",
        "workspace.rs",
    }
)

# The count after the last slice. It may only go down.
BUDGET = 1169


def production_sources() -> list[Path]:
    return sorted(
        path
        for path in GUI.rglob("*.rs")
        if "tests" not in path.relative_to(GUI).parts
        and path.name != "tests.rs"
    )


def main() -> int:
    counts: dict[str, int] = {}
    for path in production_sources():
        rel = path.relative_to(GUI).as_posix()
        counts[rel] = path.read_text(encoding="utf-8").count(TOKEN)

    failures: list[str] = []

    unknown = sorted(rel for rel, n in counts.items() if n and rel not in ALLOWED)
    for rel in unknown:
        failures.append(
            f"{rel}: mentions `{TOKEN}` but is not on the allowlist "
            "(route it through ui_kit/shortcuts or extend the allowlist)"
        )

    stale = sorted(rel for rel in ALLOWED if not counts.get(rel))
    for rel in stale:
        failures.append(
            f"{rel}: allowlisted but no longer mentions `{TOKEN}` — "
            "drop the entry, the list only shrinks"
        )

    total = sum(counts.values())
    if total > BUDGET:
        failures.append(f"total `{TOKEN}` count {total} exceeds the budget {BUDGET}")

    for failure in failures:
        print(f"FAIL {failure}")

    files = sum(1 for n in counts.values() if n)
    verdict = "PASS" if not failures else "FAIL"
    print(
        f"check_egui_surface: {verdict} — {total} `{TOKEN}` occurrences in "
        f"{files} files (budget {BUDGET})"
    )
    return 0 if not failures else 1


if __name__ == "__main__":
    sys.exit(main())
