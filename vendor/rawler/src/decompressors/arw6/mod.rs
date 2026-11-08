//! Sony ARW6 (lossy compressed RAW) decompressor.
//!
//! Ported from dnglab's rawler (`rawler/src/decompressors/arw6`,
//! LGPL-2.1, Copyright 2026 Nikolay Amiantov <ab@fmap.me>), adapted to
//! rawler 0.8.0's `PixU16` (no region-view API): tiles decode into a
//! temporary plane and are copied into the image at their record offset.

mod bitstream;
mod container;
mod geometry;
mod idwt;
mod pixelops;

use std::io::Cursor;

use bitstream_io::BitReader;

use crate::alloc_image_ok;
use crate::bits::clampbits;
use crate::pixarray::PixU16;
use crate::Result;

use bitstream::decode_component;
use container::{Region, TileRecord};
use geometry::{derive_geom, RegionGeometry, TileCoords};
use idwt::{diamond_idwt, idwt2d};
use pixelops::{delinearize, predict_horizontal};

const CODEC_BIT_DEPTH: u32 = 12;

/// BitPump for the codec's MSB-first (Big Endian) bitstreams
type BitPump<'a> = BitReader<Cursor<&'a [u8]>, bitstream_io::BigEndian>;

/// A 2-D array of wavelet coefficients.
struct Plane {
    pub w: usize,
    pub h: usize,
    pub data: Vec<i32>,
}

impl Plane {
    pub fn new(w: usize, h: usize) -> Self {
        Self {
            w,
            h,
            data: vec![0; w * h],
        }
    }

    #[inline(always)]
    pub fn row(&self, r: usize) -> &[i32] {
        &self.data[r * self.w..(r + 1) * self.w]
    }

    #[inline(always)]
    pub fn row_mut(&mut self, r: usize) -> &mut [i32] {
        let w = self.w;
        &mut self.data[r * w..(r + 1) * w]
    }
}

/// Decompress an ARW6 strip (`buf`) into the full `width` × `height` Bayer
/// mosaic.
pub(crate) fn decompress_arw6(
    buf: &[u8],
    width: usize,
    height: usize,
    dummy: bool,
) -> Result<PixU16> {
    let mut image = alloc_image_ok!(width, height, dummy);
    let records = container::parse_records(buf)?;
    for rec in &records {
        if rec.x + rec.w > width || rec.y + rec.h > height {
            return Err(format!(
                "ARW6: tile {}x{}+{}+{} exceeds the {}x{} frame",
                rec.w, rec.h, rec.x, rec.y, width, height
            )
            .into());
        }
        let regions = container::parse_tile(rec)?;
        let mut tile = vec![0u16; rec.w * rec.h];
        decode_tile_into(rec, &regions, &mut tile)?;
        for row in 0..rec.h {
            let src = &tile[row * rec.w..(row + 1) * rec.w];
            let dst_start = (rec.y + row) * width + rec.x;
            image.pixels_mut()[dst_start..dst_start + rec.w].copy_from_slice(src);
        }
    }
    Ok(image)
}

/// Reconstruct one tile into `tile`, the tile's `rec.w` × `rec.h` mosaic.
fn decode_tile_into(rec: &TileRecord<'_>, regions: &[Region<'_>], tile: &mut [u16]) -> Result<()> {
    let n_levels = regions.len() - 1; // vertical level count
    let coords = TileCoords::new(rec.w, rec.h, n_levels)?;
    let geoms = derive_geom(&coords);
    let decode_comp = |comp: usize| -> Result<Plane> {
        let mut ll = decode_single_orient(regions, &geoms, 0, comp, "LL")?;
        predict_horizontal(&mut ll);
        let mut recon = ll;
        // Detail regions 1..n_levels-1 (coarsest -> finest), each with its
        // vertical polyphase flip.
        for region in 1..n_levels {
            let detail = decode_region_comp(regions, &geoms, region, comp)?;
            let [hl, lh, hh] = &detail[..] else {
                return Err(format!(
                    "ARW6: detail region {} did not decode to 3 orientations",
                    region
                )
                .into());
            };
            recon = idwt2d(&recon, lh, hl, hh, geoms[region].vflip)?;
        }
        Ok(recon)
    };
    let mut planes = Vec::with_capacity(4);
    for comp in 0..4 {
        planes.push(if comp < 3 {
            decode_comp(comp)?
        } else {
            decode_single_orient(regions, &geoms, n_levels, 0, "green_hi")?
        });
    }
    let [green_lo, chroma_r, chroma_b, green_hi]: [Plane; 4] = planes
        .try_into()
        .map_err(|_| "ARW6: expected 4 planes")?;
    assemble_tile_into(&green_lo, &chroma_r, &chroma_b, &green_hi, rec, tile)
}

fn decode_single_orient(
    regions: &[Region<'_>],
    geoms: &[RegionGeometry],
    region_i: usize,
    comp_i: usize,
    what: &str,
) -> Result<Plane> {
    let [plane]: [Plane; 1] = decode_region_comp(regions, geoms, region_i, comp_i)?
        .try_into()
        .map_err(|orientations: Vec<Plane>| {
            format!(
                "ARW6: {} decoded to {} orientations instead of one",
                what,
                orientations.len()
            )
        })?;
    Ok(plane)
}

fn decode_region_comp(
    regions: &[Region<'_>],
    geoms: &[RegionGeometry],
    region_i: usize,
    comp_i: usize,
) -> Result<Vec<Plane>> {
    let region = &regions[region_i];
    let geom = &geoms[region_i];
    if region.components.len() != geom.n_components {
        return Err(format!(
            "ARW6: region {} carries {} components, geometry expects {}",
            region_i,
            region.components.len(),
            geom.n_components
        )
        .into());
    }
    let comp = &region.components[comp_i];
    if comp.orient_count != geom.orientations.len() {
        return Err(format!(
            "ARW6: region {} component declares {} orientations, geometry expects {}",
            region_i,
            comp.orient_count,
            geom.orientations.len()
        )
        .into());
    }
    decode_component(
        comp.data,
        geom.width,
        geom.rows_per_chunk,
        &geom.orientations,
        &comp.chunks,
    )
}

/// Combine one tile's decoded components into its Bayer mosaic.
fn assemble_tile_into(
    green_lo: &Plane,
    chroma_r: &Plane,
    chroma_b: &Plane,
    green_hi: &Plane,
    rec: &TileRecord<'_>,
    tile: &mut [u16],
) -> Result<()> {
    let mid_value = 1i32 << (CODEC_BIT_DEPTH - 1);
    let h = green_lo.h.min(green_hi.h);
    let w = green_lo.w;
    if green_hi.w < w
        || chroma_r.h < h
        || chroma_r.w < w
        || chroma_b.h < h
        || chroma_b.w < w
    {
        return Err(format!(
            "ARW6: component size mismatch: green_lo {}x{}, green_hi {}x{}, chroma_r {}x{}, chroma_b {}x{}",
            green_lo.w, green_lo.h, green_hi.w, green_hi.h, chroma_r.w, chroma_r.h, chroma_b.w, chroma_b.h
        )
        .into());
    }
    let full_w = 2 * w;
    if full_w > rec.w || 2 * h > rec.h {
        return Err(format!(
            "ARW6: {}x{} mosaic exceeds its {}x{} tile",
            full_w,
            2 * h,
            rec.w,
            rec.h
        )
        .into());
    }
    let greens = diamond_idwt(green_lo, green_hi, h, w);
    // Bayer RGGB layout per 2x2 cell: R at (even row, even col), B at (odd
    // row, odd col), greens on the other diagonal.
    for r in 0..h {
        let g_row = greens.row(r);
        let cr_row = chroma_r.row(r);
        let cb_row = chroma_b.row(r);
        let even_start = 2 * r * rec.w;
        let odd_start = (2 * r + 1) * rec.w;
        let (head, tail) = tile.split_at_mut(odd_start);
        let row_even = &mut head[even_start..even_start + rec.w];
        let row_odd = &mut tail[..rec.w];
        for i in 0..w {
            let g0 = clampbits(g_row[2 * i] + mid_value, CODEC_BIT_DEPTH); // lower-left diagonal
            let g1 = clampbits(g_row[2 * i + 1] + mid_value, CODEC_BIT_DEPTH); // upper-right diagonal
            row_odd[2 * i] = delinearize(g0);
            row_even[2 * i + 1] = delinearize(g1);
            let green_mean = (g0 as i32 + g1 as i32) >> 1;
            row_even[2 * i] = delinearize(clampbits(green_mean + 2 * cr_row[i], CODEC_BIT_DEPTH));
            row_odd[2 * i + 1] = delinearize(clampbits(green_mean + 2 * cb_row[i], CODEC_BIT_DEPTH));
        }
    }
    Ok(())
}
