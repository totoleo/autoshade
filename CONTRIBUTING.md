# Contributing to AutoShade

Bug reports and questions are welcome through the issue templates and the
Discussions tab. Pull requests are welcome too; this page is what a change has
to satisfy before it can merge, so that a contributor is not surprised by the
review.

## Build and test

```
cargo build --release
cargo test  --release --lib                                  # library battery
cargo test  --release                                        # + CLI integration and contract tests
cargo test  --release --features gui --bin autoshade-gui     # desktop front end (a test binary; it never opens a window)
cargo clippy --release --all-targets                         # must be warning-free
python scripts/check_docs.py                                 # pinned release claims re-derived from the tree
python scripts/check_text.py                                 # text resources: locales aligned, generated in step, no literal key positions
python scripts/subset_gui_fonts.py --check                   # the GUI font subsets still cover every glyph the UI can show
python scripts/check_doc_drift.py                            # every path, code symbol, link and tier count the docs name exists in the tree
```

The Python sidecars (`python/*.py`) need the packages in `python/requirements-*.txt`;
the model weights download on first use into `python/weights/` and are never
committed. Tests that need the private calibration corpus skip with a printed
reason when `AUTOSHADE_FIT_CALIBRATION_DIR` is unset.

## What a change has to carry

### Tests are behaviour acceptance

- A test asserts what the software does: state, output, rendered UI, files on
  disk. A red test must mean user-visible behaviour broke; if it can go red
  while the product still works, it is not a test.
- Do not read source text from a test: no `include_str!` of `.rs` files, no
  source-tree walks, no `contains("fn …")`, no call-site counting. Those
  punish equivalent refactors and train people to update the expectation
  mechanically.
- Architecture constraints (layer dependencies, single doors, forbidden
  APIs) belong to the compiler or to a lint, not to the test suite. The
  strength order is: types and visibility, then a lint, then a dedicated
  architecture gate. A constraint with no executor stays unenforced and the
  gap is recorded in the change description; it is not parked in the tests.

- **A root cause, not a symptom.** A fix names the mechanism it corrects and
  sweeps every site of the same class in the same change.
- **A test that would have caught it, and proof the test bites.** Every new
  invariant comes with a hand mutation that turns the test red; say which
  mutation in the PR.
- **Numbers, not adjectives.** A behaviour change in the fit, the renderer or
  the retrieval reports its before/after measurement on the documented
  fixtures. A change that regresses any of them does not ship.
- **Docs in the same change.** Rustdoc, `docs/ARCHITECTURE.md`, the README and
  the site describe the code as it is; `scripts/check_docs.py` pins the numbers
  that matter, so re-pin them to the new measurement rather than loosening the
  claim.
- **Text is a resource.** Every user-facing string is a dotted key whose English
  and Chinese text live in `resources/strings/{en,zh-Hans}.toml`; code names the
  generated `K_…` constant. Add the row to both locale files with the same
  placeholders, then run `py -3 scripts/gen_text.py`; `scripts/check_text.py`
  fails on a missing translation, a stale generated module, a placeholder
  mismatch or an English literal in a key position.

## House style

- No `cargo fmt`: the tree is not rustfmt-formatted, and the formatter would
  rewrite thousands of unrelated lines. Match the surrounding code by hand.
- Files keep their existing line endings (the tree is mixed CRLF/LF on purpose).
- No photographs from anyone's private library in the tree, in test names or in
  documentation: fixtures are synthetic or P-coded, showcase images are named by
  scene.
- No secrets, keys or machine-specific absolute paths in code; configuration
  comes from `autoshade.local.json` and `AUTOSHADE_*` environment variables.
- Commits carry no generated co-author trailers.

## Security

Vulnerabilities go through GitHub's private vulnerability reporting, not through
an issue — see [.github/SECURITY.md](.github/SECURITY.md).
