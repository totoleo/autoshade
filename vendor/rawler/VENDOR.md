# Vendored rawler 0.8.0 (LGPL-2.1)

Source: the crates.io release awler 0.8.0 (dnglab), plus the ARW6
decompressor from dnglab's main branch:

- src/decompressors/arw6/{mod,bitstream,container,geometry,idwt,pixelops}.rs
  and delin_curve.csv: Copyright 2026 Nikolay Amiantov <ab@fmap.me>,
  LGPL-2.1, taken from dnglab awler/src/decompressors/arw6.
- src/decoders/arw.rs: compression 32766 now decodes through that codec
  (black/white levels doubled from the raw IFD, codec-internal curve), and
  the camera lookup is mode-less as in dnglab main — the released 0.8.0
  looked cameras up under mode "arw6", which no Sony entry has, so Sony's
  lossy-compressed ARW (A7 V among them) was refused as "Unknown camera".
- src/decompressors/arw6/mod.rs was adapted to rawler 0.8.0's PixU16
  (no image-region API): tiles decode into a temporary plane and are copied
  into the image at their record offset.

This crate remains LGPL-2.1; see LICENSE.

## Upstream status of ARW6 (checked 2026-11-08)

The port above was made because no Rust decoder could open these files, and the
notes below are what that claim looks like now. Dated, because two of the three
changed within four days of the port:

- **LibRaw snapshot 202611 (0.23, 2026-11-04)** ships
  `src/decoders/sony_arw6.cpp` (`sony_arw6_load_raw()`), and its camera list
  gained `ILCE-7M5 (A7 V)`. ARW 6.0 compressed and compressed (HQ) decode there.
- **reference stable 5.6.2** does NOT use LibRaw and still refuses the files
  (measured: `[rawspeed] … Unsupported compression 32766`). Its release notes
  list "Sony ARW 6.0 compressed and compressed (HQ)" under *Missing Compression
  Mode Support*.
- **reference master** merged a LibRaw fallback on 2026-11-07 (PR #23000,
  milestone 5.8): when RawSpeed fails, files whose LibRaw decoder is
  `sony_arw6_load_raw()` load through LibRaw (tested there with ILCE-7M5,
  black 1024 / white 32800). The first nightly after that date decodes ARW6;
  the 5.6.x line does not.
- **RawSpeed's own ARW6 decoder** (PR #983) is still open and unmerged, which is
  why the reference fix had to go through LibRaw instead.

None of this changes this vendored crate: AutoShade links rawler (Rust), not
LibRaw and not reference, and this port is still the only ARW6 decoder in its
tree. The exit condition is unchanged — when a released rawler/dnglab carries
ARW6, delete `vendor/rawler` and the `[patch.crates-io]` entry, and bump the
crate version.
