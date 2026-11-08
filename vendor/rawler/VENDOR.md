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
