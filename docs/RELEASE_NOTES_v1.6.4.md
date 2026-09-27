# AutoShade v1.6.4 — the desktop panels: every button in the grid, the histogram at the readable width, and a painted area for each tool

Everything merged into `main` since v1.6.3 ships here: three desktop-app
defects the maintainer met using v1.6.3 on 2026-09-27, each decided on a probe
of every button the panels draw, and the site's home page cut to two thirds of
its words. The library and the command line are v1.6.3's in source — every
commit since that tag touches the desktop app's panels, the documents, the
site or the tests — so a recipe, a render or a fit is v1.6.3's. Nothing new is
added — this is a patch release — and every change below carries the
measurement that decided it.

The design, the runs, the acceptance and this document are the maintainer's;
no external coding model wrote this release.

## The desktop app

- **Every text button in the side panels is laid in a grid cell.** The panels
  mixed two kinds of button: rows of equal verbs laid in cells of one width,
  and single verbs sized by their own label, so the rows of a panel ended at
  different right edges; and the AI section's body was indented 18 px by its
  outer fold, so its rows started 18 px right of Develop's and Retouch's with
  cells 9 px narrower. Now a row's one verb fills the row, two or three share
  it equally, a verb beside a checkbox or a label takes one cell of a
  two-column grid measured at the row's start, and the AI section's body is
  laid unindented, so the three sections' rows start on one x and end on one
  right edge. The version row's 「Load」 and the gallery header's 「🗂 Open
  folder…」 keep their own widths (list and header verbs), and so do the
  toolbar, the dialogs and Settings, as toolbars do. The crop fold's aspect
  preset moves under its two verbs on a captioned row. A GUI test lays every
  panel out in both languages over three frames and pins each side-panel text
  verb to a cell and the three sections to one left edge.
- **The Develop histogram keeps the readable ceiling.** The readout grew with
  the side panel without limit; it now stops at the 420 px ceiling the prompt
  fields and the button rows already keep. The every-button test fixture
  carries a histogram now, so the readable-width test pins the readout at 800
  and 1600 px panels too.
- **Each brush tool keeps its own painted area.** Generative Fill, Heal, Clone
  Stamp and the AI panel's Adjust fold shared one 「Brush」 group above the
  Retouch folds and one painted area, and the Adjust fold read that area with
  no control of its own. Now each fold carries its own 「🖌 Paint area」 / 「Clear
  area」 row with the brush-size slider under whichever brush is armed (the
  Stamp arms through 「⎘ Enter stamp」), and each tool keeps its own strokes:
  what is painted for Heal is not what Fill, the Stamp or Adjust read; a Local
  Masks brush session sets the live area aside and gives it back; a landing
  clears only the area its result consumed; the K key toggles the brush of the
  tool on the canvas, and the canvas caption names that tool's area. Two GUI
  tests pin it: each tool's strokes survive another tool's turn without a
  rescan, clearing one area leaves the others, a mask session stashes and
  restores, a plate rebind starts every tool blank; and arming a brush sweeps
  the other canvas tools while putting it away ends a live mask session
  instead of orphaning it.

## Documents and the site

- **The manual**'s Generative Fill, Heal, Clone Stamp and Adjust paragraphs
  say the brush is per tool and where its row lives; **ARCHITECTURE**'s Adjust
  paragraph follows, and its count paragraph names the three GUI tests added
  and the one replaced. Nine interface strings are added and eight retired, in
  both languages.
- **The site**'s home page (deployed 2026-09-27, before this release) reads
  1,806 words where it read 2,706: the overview's twelve cards are nine, the
  three retrieval cards under Part B are retired in favour of the pillar that
  carries the same mathematics, every showcase caption is one or two lines,
  the header's nine entries are seven on both pages, and the technology
  section's headings are smaller. Every number was kept or deleted, none
  altered.

## Compatibility

No recipe, sidecar, store or weight format changes. The sidecar weights stay
pinned to the v1.6.0 `autoshade-raw-denoise-v2.pth`; no weights are
re-shipped. What changes on screen: the 「Brush」 group above the Retouch folds
is gone and each fold's 「🖌 Paint area」 row stands in its place; the K key
toggles the brush of the tool whose fold is on the canvas, where it toggled
the one shared paint mode; strokes painted for one tool no longer show up in
another's; every side-panel row ends on the same right edge; the Develop
histogram is at most 420 px wide.

## Gates

Measured before the tag on the release code (`5ed926d`; no file under `src/`
outside the desktop app changed since v1.6.3). A first run, on `e83cf7d`, read
the same counts in every lane and failed clippy on one lint in the GUI test
file (a field assigned after `Default::default()`); the one commit between
fixes that line, and this run is the record. The version bump touches
Cargo.toml, Cargo.lock, the documents, the site's cache keys and the bug
template's dropdown, and the CLI, contract and doc-test suites, the denoise
module, clippy, the `python/` suite and `check_docs` are re-run after it. The
three-lane release battery (`scripts/release_battery.sh`, a frozen snapshot
worktree, the p36–p41 calibration corpus and the sidecar weights in reach):
library **1752 passed / 0 failed / 15 ignored** (768.66 s, release profile,
one process per module), CLI **27 / 0**, contract 2 + 2, doc-tests 0, GUI
**224 / 0 / 1**, calibration lane **1752 / 0 / 15** (1013.59 s; one skip line,
the mask-brush specimen test whose `AUTOSHADE_MB_SAMPLE_ROOT` specimen is not
on this machine, named in every release since v1.3.2), `audit_i18n` and the
font check exit 0 inside the battery; the Python suites 81 OK from `python/`
and 44 OK from `scripts/` (CPU, the real weights, `-W error::RuntimeWarning`).
By name against the v1.6.3 release battery: library 1767 → 1767, nothing added
or removed; CLI 27 → 27, nothing added or removed; GUI 223 → 225, the three
additions being
`arming_a_brush_sweeps_the_other_tools_and_putting_it_away_ends_a_mask_session`;
`each_tool_keeps_its_own_painted_area`;
`every_side_panel_text_verb_is_laid_in_a_cell_and_the_sections_align` and the
one retirement
`un_ticking_paint_mask_ends_the_brush_session_instead_of_orphaning_it`, the
pin on the retired 「Paint mask」 checkbox that the third addition replaces.
clippy 0 warnings on both feature sets (`--all-targets -- -D warnings`).
`check_docs.py --gates` on the transcript with the XMP census root supplied:
on the frozen snapshot **32 PASS / 0 FAIL / 0 SKIP** — the counts moved with
the code before the snapshot — and after the bump **32 PASS / 0 FAIL / 0
SKIP**; after the bump the CLI, contract and doc-test suites read 27 / 0, 2 +
2 and 0, the denoise module 47 / 0, clippy 0 warnings on both feature sets,
the `python/` suite 81 OK, and `audit_i18n` and the font check exit 0.
Photo-name, token-shape and user-path greps on every commit of the release: 0
/ 0 / 0 over 9 scans (8 commits and the working tree).

Final gate, reference pair, before the tag: the release code's CLI re-fitted
the reference pair at 0.65 / 0.85 / 1.0 and rendered each at the target's
size. At every strength the recipe (store paths normalised), every mask raster
and both renders, with and without the masks, are byte for byte the v1.6.3
gate's — themselves the v1.6.1 gate's approved ones, through v1.6.2 — so the
readings stand: sky ΔE / whole-frame mean |diff| 6.3 / 0.0283, 3.8 / 0.0248
and 3.8 / 0.0249 against the target. Nothing in this release reaches this
path: the library and the command line are v1.6.3's in source.

Not measured: no paid image call was made in this release's gates (the
maintainer used 「✨ Adjust」 — the paid image edit — on the installed v1.6.3 on
2026-09-27 and it returned a card, the fold's first use since it shipped in
v1.5.1); the GUI executable was not launched in the release chain — the three
changes are pinned by GUI tests that lay the panels out headlessly in both
languages, and the panels' pixels were not read from a live window; the
sharpening amount's scale against Lightroom's, listed since v1.6.0, is still
unmeasured — the three Lightroom exports it needs were made on 2026-09-27 and
the measurement is the next piece of work, not this release's. The ship facts
(the release run, the downloaded assets, the site, the local upgrade) are in
the ROADMAP ledger entry.
