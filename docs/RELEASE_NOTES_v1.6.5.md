# AutoShade v1.6.5 — the Sharpening slider measured against Lightroom's

Everything merged into `main` since v1.6.4 ships here: one change to the
develop engine, the capture-sharpening operator, whose scale and shape are now
fitted to Lightroom 9.4's own output on a measured frame — the item every
release since v1.6.0 listed as designed and not yet shipped. The desktop app,
the command line and the sidecar layer are v1.6.4's in source; every render
that sharpens (a RAW at Lightroom's default of 40, or any recipe with a
Sharpness above 0) moves by this law now. Every number below is read from the
calibration lane's records.

The design, the measurement, the fit and this document are the maintainer's;
no external coding model wrote this release.

## The develop engine

- **The sharpening law is measured, not designed.** On 2026-09-27 the
  maintainer exported one 61 MP frame (ILCE-7RM4A, ISO 2500, 20 s, a star
  field over dark ground, Lightroom's Denoise 50 on) from Lightroom 9.4 at
  Sharpness 0, 40 and 80 with Radius 1.0, Detail 25 and Masking 0, and the
  operator's constants were fitted to what moved between the exports, per
  pixel, in the exports' gamma-encoded luma, on a sample stratified over local
  luminance × signal amplitude × side of the edge. The fit explains **R² 0.93
  of the move at 40 and 0.92 at 80** over the whole frame and **0.95 on the
  pixels whose unsharp signal is past 0.03** — the edges and the stars one
  sees — where the plain unsharp mask it replaces explained 0.62. The engine
  reproduces the law on its own renders at R² 0.998 (the fitted law applied in
  Python to the engine's Sharpness-0 render against the engine's own 40 and 80
  renders, on the saved denoised master and on the untouched RAW alike).
- **What the frame read, and what the law does.** Radius 1.0 is a 0.75 px
  Gaussian (the limiters broaden the apparent kernel to the 1.17 px a plain fit
  reads off the export). The gain on the unsharp signal is about linear in the
  slider on both sides of an edge — 1.9 per unit on the light side and 2.5 on
  the dark side at Amount 40 — and rolls off in the shadows (half at a local
  luminance of 0.014 in gamma 2.2, Hill slope 2.8: the frame's dark ground took
  0.03 of the sky's gain, so shadow noise is left alone). Each side is
  soft-limited on the quadratic-mean knee `f / √(1 + (f / cap)²)`: the light
  side at a constant 0.145 (growing as (Amount/40)^0.21), the dark side at
  `1.1 × Y₀ − 0.07 × f` with `Y₀` the pixel's OWN luminance — Lightroom's dark
  halo is bounded by the pixel it darkens, not by one constant. The ring
  around a star therefore dips with the star: Lightroom's exports dipped 0.02
  / 0.04 / 0.08 for faint / mid / bright peaks at 40 and the law reads 0.02 /
  0.04 / 0.07; star peaks lifted +0.07 / +0.11 / +0.12 there against the law's
  +0.08 / +0.11 / +0.13, and +0.13 / +0.15 / +0.13 at 80 against +0.13 / +0.15
  / +0.16. A ring below an encoded luma of about 0.15 does not dip at all. The
  move is added to R, G and B alike, as the exports moved (Lightroom's export
  moved the two chroma differences by 0.13 and 0.38 of the luma move, where
  chroma scaling would have moved them 0.69 and 0.89).
- **On the engine's own render of the same frame** (the saved denoised master,
  Sharpness 40) the star peaks now lift +0.07 / +0.10 / +0.12 and the rings
  dip 0.03 / 0.05 / 0.07 for the three classes — Lightroom's own export read
  +0.07 / +0.11 / +0.12 and 0.02 / 0.04 / 0.08. The v1.6.4 operator lifted
  +0.06 / +0.08 / +0.09 and dipped a flat 0.02 whatever the star.
- **What was not measured.** Every export was at Masking 0 (the edge gate keeps
  its designed ramp), at Detail 25 (the caps are carried across the Detail band
  on the designed log ramp: no cap at 100, a little tighter at 0), at Radius
  1.0 and at Amounts up to 80; Detail's other values, other radii and Amounts
  past 80 are the law's own extrapolation. One frame, one camera, one ISO. The
  exports were made with Lightroom's Denoise on, so the noise regime the law
  was fitted in is a denoised one; the engine's untouched-RAW render sharpens
  its noise by the same law. A local mask's negative Sharpness (a softening
  brush) is still the plain signed unsharp mask, unmeasured.
- **A test pins the law's distinctive behaviour**: a dim step (0.10 → 0.40) and
  a bright step (0.40 → 0.70) of one height at Sharpness 40 — the dark side of
  the dim step hardly dips, the dark side of the bright step dips by an order
  more, and both light sides lift alike.

## Documents and the site

- **README**'s parity section names the sharpening law as the fifth measured
  one and its measured-numbers table carries its row; the designed-not-shipped
  item that listed the calibration since v1.6.0 is retired. **TECH_STACK**'s
  Sharpening bullet is the law with every constant; **ARCHITECTURE**'s follows
  in prose; **the manual** says what the slider now does. The site changes only
  its version words, its battery line and its cache keys.

## Compatibility

No recipe, sidecar, store or weight format changes. The sidecar weights stay
pinned to the v1.6.0 `autoshade-raw-denoise-v2.pth`; no weights are
re-shipped. What changes on screen: every render with a Sharpness above 0 —
which is every RAW develop, at Lightroom's default of 40 unless the recipe
says otherwise — re-renders under the measured law: light edges and star cores
lift about a third more than under v1.6.4, the dark halo beside a bright edge
digs as deep as Lightroom's where v1.6.4 held it at a flat 0.02, and deep
shadows move less. A recipe's numbers are untouched; only what a number does
to the pixels changed.

## Gates

Measured before the tag on the release code (`4aaf923`; three files under
`src/` outside the desktop app changed since v1.6.4 — `render/detail.rs`, the
law; `render.rs`, the additive luma write; `render/detail/tests.rs`, the
test — and nothing under the desktop app). The version bump touches
Cargo.toml, Cargo.lock, the documents, the site's cache keys and the bug
template's dropdown, and the CLI, contract and doc-test suites, the denoise
module, clippy, the `python/` suite and `check_docs` are re-run after it. The
three-lane release battery (`scripts/release_battery.sh`, a frozen snapshot
worktree, the p36–p41 calibration corpus and the sidecar weights in reach):
library **1753 passed / 0 failed / 15 ignored** (894.94 s, release profile,
one process per module), CLI **27 / 0**, contract 2 + 2, doc-tests 0, GUI
**224 / 0 / 1**, calibration lane **1753 / 0 / 15** (1567.28 s; one skip line,
the mask-brush specimen test whose `AUTOSHADE_MB_SAMPLE_ROOT` specimen is not
on this machine, named in every release since v1.3.2), `audit_i18n` and the
font check exit 0 inside the battery; the Python suites 81 OK from `python/`
and 44 OK from `scripts/` (CPU, the real weights, `-W error::RuntimeWarning`).
By name against the v1.6.4 release battery: library 1767 → 1768, one addition,
`the_dark_halo_digs_with_the_pixels_own_luminance`, nothing removed; CLI
27 → 27, nothing added or removed; GUI 225 → 225, nothing added or
removed. clippy 0 warnings on both feature sets (`--all-targets -- -D
warnings`). `check_docs.py --gates` on the transcript with the XMP census root
supplied: on the frozen snapshot **32 PASS / 0 FAIL / 0 SKIP** — the counts moved
with the code before the snapshot — and after the bump [[POSTBUMP_CD]].
Photo-name, token-shape and user-path greps on every commit of the release:
[[GREPS]].

Final gate, reference pair, before the tag: the release code's CLI re-fitted
the reference pair at 0.65 / 0.85 / 1.0 and rendered each at the target's
size. This time the recipes and the mask rasters are not the v1.6.4 gate's, and the cause is the law itself: a RAW whose recipe holds no Sharpening renders at Lightroom's default of 40 (`capture_sharpening` in `recipe.rs`), so every render the reverse fit compares with the target — the base look's, the colour field's, the zone evidence's — carries the measured law now. The solver's path moved with them: the base curve by at most 0.0015, the colour field's cells by 0.001 on average and 0.075 at most, and the two land bands the solver had admitted at 0.85 and 1.0 in v1.6.4 were not admitted this time — at 0.65 v1.6.4 admitted none either — so every strength now carries the same five masks (two sky bands, two spatial tiles, the field zone); the look error at 0.85 read 0.022 → 0.021. The two 1000 px renders differ from the v1.6.4 gate's bytes on that account and on the operator's own: at 1000 px the 0.75 px film σ is 0.08 px of the raster, under the 0.6 px floor, so the operator runs at the floor with 0.04 of its amount, and that residue now follows the measured law. The readings, sky ΔE / whole-frame mean |diff| against the target, v1.6.4 → v1.6.5: 0.65: 6.3 / 0.0283 → 6.3 / 0.0284, 0.85: 3.8 / 0.0248 → 3.9 / 0.0250, 1.0: 3.8 / 0.0249 → 3.9 / 0.0252. The maintainer's own look at the three-way sheet at 0.85 (the target, the v1.6.1 approved gate, this build): the sky is one smooth gradient with no block, seam or tile, the ground reads as the approved gate's, and the 1000 px renders at the three strengths are the same picture as v1.6.4's to the eye. One mutation falsified the new test on the snapshot: with the dark cap's luminance slope set to 100 — a cap that never binds — `the_dark_halo_digs_with_the_pixels_own_luminance` failed on its dim-step assertion, and the file was restored to its committed bytes (SHA-256 matched, worktree clean). After the snapshot one module comment at the head of `render.rs`, the sentence on the Detail panel, was corrected to say the sharpening is measured; the gates re-run after the bump compile it.
