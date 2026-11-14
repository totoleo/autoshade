//! The recipe library: named develops kept OUTSIDE any one photo.
//!
//! One file per recipe, `<store>/recipes/<name>.json`, in the same shape
//! `recipe.json` holds — so `autoshade apply <raw> <library file>` reads one
//! unchanged and the GUI and the CLI share a single format. The store root is
//! [`crate::store::store_root`]'s (`AUTOSHADE_DATA_DIR` moves the library with
//! the develops it was lifted from).
//!
//! A library entry is deliberately NOT the develop of the photo it was saved
//! from: it is the part of that develop that means the same thing on another
//! photograph. [`portable`] is that shaping, and it is the SAME rule the
//! clipboard paste follows (the GUI's paste payload calls it) — the masks,
//! layers and calibration whose pixels belong to one frame are dropped, and
//! the count of what was dropped rides back to the caller for disclosure.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::recipe::{EditRecipe, MaskGeometry};

/// Directory under the store root that holds the library.
pub const DIR_NAME: &str = "recipes";

pub fn library_dir() -> PathBuf {
    library_dir_in(&crate::store::store_root())
}

pub fn library_dir_in(root: &Path) -> PathBuf {
    root.join(DIR_NAME)
}

pub fn entry_path(name: &str) -> PathBuf {
    entry_path_in(&crate::store::store_root(), name)
}

pub fn entry_path_in(root: &Path, name: &str) -> PathBuf {
    library_dir_in(root).join(format!("{name}.json"))
}

/// Is a recipe with this name already in the library?
pub fn exists(name: &str) -> bool {
    entry_path(name).is_file()
}

/// What [`portable`] took OUT of a develop because it belongs to the photo it
/// was made on. Every field is a disclosure the caller is expected to say: a
/// silent drop is how a library entry quietly stops looking like the photo it
/// was saved from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Losses {
    /// Masks whose pixels are the source frame's own (bitmap rasters, and the
    /// reverse-fit zones that carry one).
    pub masks: usize,
    /// The per-photo spatial colour field.
    pub colour_field: bool,
    /// Fill layers — the source frame's own pixels.
    pub layers: usize,
    /// The AI lens blur, whose depth map measures the source scene.
    pub lens_blur: bool,
    /// Crop / straighten, when the caller asked for a look without geometry.
    pub geometry: bool,
}

impl Losses {
    pub fn is_empty(&self) -> bool {
        self.masks == 0
            && !self.colour_field
            && self.layers == 0
            && !self.lens_blur
            && !self.geometry
    }
}

/// `recipe` shaped to travel: the edit, and nothing that belongs to the source
/// photo. `geometry` false also drops crop + straighten (composition rarely
/// transfers between frames).
///
/// The one implementation of this rule: the GUI's clipboard paste computes its
/// foreign payload through here, and so does saving into the library.
pub fn portable(recipe: &EditRecipe, geometry: bool) -> (EditRecipe, Losses) {
    let mut out = recipe.clone();
    let mut losses = Losses::default();
    if !geometry {
        losses.geometry = out.crop.is_some() || out.straighten_deg != 0.0;
        out.crop = None;
        out.straighten_deg = 0.0;
    }
    // Bitmap masks reference per-photo rasters keyed to the SOURCE stem —
    // carried to another photo they point at the wrong file (and classic XMP
    // cannot carry them, so recipe.json and .xmp would disagree).
    //
    // The reverse-fit's sky/land ZONES travel no better, and they are AI masks
    // now (`MaskGeometry::select_sky`), so the variant test alone would carry
    // a stale raster onto a photo it was never segmented for. The distinction
    // is `MaskRole::is_zone`: an IMPORTED Lightroom AI mask carries only
    // intent, and `segment::resolve_ai_masks` re-derives it against whatever
    // photo it lands on, so that one still travels.
    // Four-gradient tiles carry their complete geometry and can travel; a
    // refined Bitmap anywhere in the composition still belongs to its photo.
    let photo_keyed = |m: &crate::recipe::LocalAdjustment| {
        std::iter::once(&m.mask).chain(m.components.iter().map(|c| &c.geometry)).any(|g| {
            matches!(g, MaskGeometry::Bitmap { .. })
                || (m.role.is_zone() && crate::render::geometry_raster_path(g).is_some())
        })
    };
    losses.masks = out.masks.iter().filter(|m| photo_keyed(m)).count();
    out.masks.retain(|m| !photo_keyed(m));
    // The R25 B4 PASS-THROUGH map is PER-DOCUMENT state, not a setting: it is
    // Lightroom's own Transform block and camera profile name read off ONE
    // document. An empty map STRIPS nothing, so every other photo's own block
    // survives untouched in its own sidecar and there is no loss to report.
    out.passthrough.clear();
    // A colour field is PER-PHOTO in the strongest sense a control in this
    // recipe can be: its twelve-by-eight spatial cells are normalised to the
    // frame it was solved on, so cell (3, 1) means "this scene's left-of-centre
    // sky" and nothing at all on another photograph.
    losses.colour_field = out.colour_field.take().is_some();
    // A fill layer is the source frame's own pixels at the source frame's
    // coordinates.
    losses.layers = std::mem::take(&mut out.pixel_layers).len();
    // A lens blur's depth map is the source scene's distances.
    losses.lens_blur = out.lens_blur.take().is_some();
    (out, losses)
}

/// One library row. A file that exists but does not parse is LISTED with its
/// error rather than hidden: a missing row is indistinguishable from a recipe
/// that was never saved, and the user would only find out at apply time.
#[derive(Debug)]
pub struct Stored {
    pub name: String,
    pub modified: Option<SystemTime>,
    /// Masks the stored recipe carries (0 when it does not parse).
    pub masks: usize,
    /// `Some` when the file exists but is not a readable recipe.
    pub error: Option<String>,
}

pub fn list() -> Vec<Stored> {
    list_in(&crate::store::store_root())
}

pub fn list_in(root: &Path) -> Vec<Stored> {
    let dir = library_dir_in(root);
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out: Vec<Stored> = Vec::new();
    for entry in rd.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()).is_none_or(|e| !e.eq_ignore_ascii_case("json")) {
            continue;
        }
        let Some(name) = path.file_stem().and_then(|s| s.to_str()).map(str::to_owned) else {
            continue;
        };
        let modified = entry.metadata().ok().and_then(|m| m.modified().ok());
        let parsed = crate::store::read_text_capped(&path, crate::store::MAX_STORE_JSON)
            .map_err(|e| e.to_string())
            .and_then(|text| serde_json::from_str::<EditRecipe>(&text).map_err(|e| e.to_string()));
        out.push(match parsed {
            Ok(recipe) => Stored { name, modified, masks: recipe.masks.len(), error: None },
            Err(e) => Stored { name, modified, masks: 0, error: Some(e) },
        });
    }
    out.sort_by_cached_key(|s| (s.name.to_lowercase(), s.name.clone()));
    out
}

pub fn read(name: &str) -> Result<EditRecipe> {
    read_in(&crate::store::store_root(), name)
}

pub fn read_in(root: &Path, name: &str) -> Result<EditRecipe> {
    let path = entry_path_in(root, name);
    let text = crate::store::read_text_capped(&path, crate::store::MAX_STORE_JSON)
        .with_context(|| format!("recipe 「{name}」 could not be read ({})", path.display()))?;
    let mut recipe: EditRecipe = serde_json::from_str(&text)
        .with_context(|| format!("recipe 「{name}」 is not a recipe file ({})", path.display()))?;
    // A hand-imported file may name its rasters, like a store recipe does.
    if let Some(base) = path.parent() {
        crate::store::resolve_mask_paths(&mut recipe, base);
    }
    Ok(recipe)
}

fn write_at(path: &Path, recipe: &EditRecipe) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("the recipe library folder could not be created ({})", dir.display()))?;
    }
    let body = serde_json::to_vec_pretty(recipe).context("the recipe could not be serialised")?;
    crate::store::durable_write(path, &body)
        .with_context(|| format!("the recipe could not be written ({})", path.display()))
}

/// Write a NEW entry. Refuses a taken name: an accidental overwrite of a look
/// the user cannot get back is the one failure this library must not have.
pub fn create(name: &str, recipe: &EditRecipe) -> Result<()> {
    create_in(&crate::store::store_root(), name, recipe)
}

pub fn create_in(root: &Path, name: &str, recipe: &EditRecipe) -> Result<()> {
    let path = entry_path_in(root, name);
    if path.exists() {
        anyhow::bail!("a recipe named 「{name}」 already exists");
    }
    write_at(&path, recipe)
}

/// Replace an EXISTING entry's content, keeping its name.
pub fn replace(name: &str, recipe: &EditRecipe) -> Result<()> {
    replace_in(&crate::store::store_root(), name, recipe)
}

pub fn replace_in(root: &Path, name: &str, recipe: &EditRecipe) -> Result<()> {
    let path = entry_path_in(root, name);
    if !path.is_file() {
        anyhow::bail!("no recipe named 「{name}」 in the library");
    }
    write_at(&path, recipe)
}

/// Rename an entry (the file moves with it). Refuses a taken name.
pub fn rename(from: &str, to: &str) -> Result<()> {
    rename_in(&crate::store::store_root(), from, to)
}

pub fn rename_in(root: &Path, from: &str, to: &str) -> Result<()> {
    let src = entry_path_in(root, from);
    let dst = entry_path_in(root, to);
    if !src.is_file() {
        anyhow::bail!("no recipe named 「{from}」 in the library");
    }
    if dst.exists() {
        anyhow::bail!("a recipe named 「{to}」 already exists");
    }
    std::fs::rename(&src, &dst)
        .with_context(|| format!("the recipe could not be renamed ({})", src.display()))
}

pub fn delete(name: &str) -> Result<()> {
    delete_in(&crate::store::store_root(), name)
}

pub fn delete_in(root: &Path, name: &str) -> Result<()> {
    let path = entry_path_in(root, name);
    std::fs::remove_file(&path)
        .with_context(|| format!("the recipe could not be deleted ({})", path.display()))
}

/// Read a recipe FILE the user picked, `.json` (this app's own recipe shape) or
/// `.xmp` (a Lightroom / Camera Raw sidecar, through the same importer the
/// desktop app runs when it opens a photo).
pub fn read_file(path: &Path) -> Result<EditRecipe> {
    let text = crate::store::read_text_capped(path, crate::store::MAX_STORE_JSON)
        .with_context(|| format!("{} could not be read", path.display()))?;
    let is_xmp = path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("xmp"));
    if is_xmp {
        let (recipe, _) = crate::xmp::xmp_to_recipe_clamped(&text);
        Ok(recipe)
    } else {
        serde_json::from_str(&text)
            .with_context(|| format!("{} is not a recipe.json or an .xmp sidecar", path.display()))
    }
}

/// Why a name cannot be a library entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameError {
    Empty,
    TooLong,
    /// A character the file system (or a path) cannot hold.
    Illegal(char),
}

/// The file-system name a typed recipe name becomes, or why it cannot.
pub fn check_name(raw: &str) -> std::result::Result<String, NameError> {
    let name = raw.trim();
    if name.is_empty() {
        return Err(NameError::Empty);
    }
    if name.chars().count() > crate::store::MAX_STORE_NAME {
        return Err(NameError::TooLong);
    }
    if let Some(c) = name
        .chars()
        .find(|c| c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'))
    {
        return Err(NameError::Illegal(c));
    }
    // Windows drops a trailing dot or space from a file name, so two names the
    // user sees as different would land on one file.
    if let Some(c) = name.chars().last().filter(|c| *c == '.' || *c == ' ') {
        return Err(NameError::Illegal(c));
    }
    Ok(name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipe::{LocalAdjustment, MaskRole};

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "autoshade-recipes-{tag}-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn recipe() -> EditRecipe {
        EditRecipe { exposure_ev: 0.42, contrast: 12.0, ..EditRecipe::default() }
    }

    #[test]
    fn the_library_writes_reads_renames_and_deletes_one_file_per_recipe() {
        let root = temp_root("crud");
        create_in(&root, "Late sun", &recipe()).unwrap();
        assert!(entry_path_in(&root, "Late sun").is_file());
        // The file is a plain recipe: the CLI's `apply` can read it.
        let raw = std::fs::read(entry_path_in(&root, "Late sun")).unwrap();
        let parsed: EditRecipe = serde_json::from_slice(&raw).unwrap();
        assert_eq!(parsed.exposure_ev, 0.42);

        assert_eq!(list_in(&root).len(), 1);
        assert_eq!(list_in(&root)[0].name, "Late sun");
        assert_eq!(read_in(&root, "Late sun").unwrap().contrast, 12.0);

        replace_in(&root, "Late sun", &EditRecipe { contrast: 33.0, ..EditRecipe::default() }).unwrap();
        assert_eq!(read_in(&root, "Late sun").unwrap().contrast, 33.0);
        assert_eq!(list_in(&root)[0].name, "Late sun", "a replace keeps the name");

        rename_in(&root, "Late sun", "Winter").unwrap();
        assert!(!entry_path_in(&root, "Late sun").exists());
        assert!(entry_path_in(&root, "Winter").is_file());
        assert_eq!(list_in(&root)[0].name, "Winter");

        delete_in(&root, "Winter").unwrap();
        assert!(list_in(&root).is_empty());
    }

    #[test]
    fn a_taken_name_is_refused_rather_than_overwritten() {
        let root = temp_root("taken");
        create_in(&root, "Look", &recipe()).unwrap();
        assert!(create_in(&root, "Look", &EditRecipe::default()).is_err());
        assert_eq!(read_in(&root, "Look").unwrap().exposure_ev, 0.42, "the first one survived");
        create_in(&root, "Other", &recipe()).unwrap();
        assert!(rename_in(&root, "Other", "Look").is_err());
        assert!(entry_path_in(&root, "Other").is_file());
    }

    #[test]
    fn an_unreadable_file_is_listed_with_its_error_not_hidden() {
        let root = temp_root("broken");
        create_in(&root, "Good", &recipe()).unwrap();
        std::fs::write(entry_path_in(&root, "Broken"), b"{ not json").unwrap();
        let rows = list_in(&root);
        assert_eq!(rows.len(), 2, "a corrupt entry is a row: {rows:?}");
        let broken = rows.iter().find(|r| r.name == "Broken").unwrap();
        assert!(broken.error.is_some());
        assert!(rows.iter().find(|r| r.name == "Good").unwrap().error.is_none());
    }

    #[test]
    fn a_missing_library_is_an_empty_list_not_an_error() {
        let root = temp_root("empty");
        assert!(list_in(&root).is_empty());
        assert!(!exists("no such recipe"));
    }

    #[test]
    fn names_a_file_cannot_hold_are_refused() {
        assert_eq!(check_name("  Warm  ").unwrap(), "Warm");
        assert_eq!(check_name("a/b"), Err(NameError::Illegal('/')));
        assert_eq!(check_name("a:b"), Err(NameError::Illegal(':')));
        assert_eq!(check_name("dot."), Err(NameError::Illegal('.')));
        assert_eq!(check_name("   "), Err(NameError::Empty));
        assert_eq!(check_name(&"x".repeat(crate::store::MAX_STORE_NAME + 1)), Err(NameError::TooLong));
    }

    #[test]
    fn portable_drops_what_belongs_to_the_source_photo_and_counts_it() {
        let mut r = recipe();
        r.crop = Some(crate::recipe::Crop { left: 0.1, top: 0.1, right: 0.9, bottom: 0.9 });
        r.straighten_deg = 1.5;
        r.colour_field = Some(serde_json::from_str("{}").unwrap());
        r.pixel_layers = vec![serde_json::from_str("{}").unwrap()];
        r.lens_blur = Some(serde_json::from_str("{}").unwrap());
        r.masks = vec![
            LocalAdjustment {
                mask: MaskGeometry::Bitmap { path: "mask.png".into() },
                ..LocalAdjustment::default()
            },
            LocalAdjustment {
                mask: MaskGeometry::Linear { zero_x: 0.0, zero_y: 0.0, full_x: 1.0, full_y: 1.0 },
                ..LocalAdjustment::default()
            },
        ];
        let (out, losses) = portable(&r, false);
        assert_eq!(losses.masks, 1);
        assert!(losses.colour_field);
        assert_eq!(losses.layers, 1);
        assert!(losses.lens_blur);
        assert!(losses.geometry);
        assert!(out.crop.is_none() && out.straighten_deg == 0.0);
        assert_eq!(out.masks.len(), 1, "the parametric gradient travels");
        assert!(out.colour_field.is_none() && out.pixel_layers.is_empty() && out.lens_blur.is_none());
        assert!(!losses.is_empty());

        let (same, losses) = portable(&recipe(), true);
        assert!(losses.is_empty(), "a plain recipe loses nothing: {losses:?}");
        assert_eq!(same.exposure_ev, 0.42);
    }

    #[test]
    fn a_zone_mask_without_its_raster_travels_and_one_with_it_does_not() {
        let mut zone = LocalAdjustment::default();
        zone.role = MaskRole::ZoneSky;
        zone.mask = MaskGeometry::AiMask {
            name: "Sky 1".into(),
            subtype: 2,
            ref_x: 0.5,
            ref_y: 0.5,
            blend_mode: 0,
            value: 1.0,
            inverted: false,
            mask_version: 1,
            provenance: Vec::new(),
            gesture: Vec::new(),
            raster: None,
        };
        let mut plain = recipe();
        plain.masks = vec![zone.clone()];
        let (kept, losses) = portable(&plain, false);
        assert_eq!(kept.masks.len(), 1, "an intent-only zone is re-derived per photo");
        assert_eq!(losses.masks, 0);

        // With its raster the zone is the source photo's measured selection.
        let mut with_raster = recipe();
        zone.mask = MaskGeometry::Bitmap { path: "zone.png".into() };
        with_raster.masks = vec![zone];
        let (dropped, losses) = portable(&with_raster, false);
        assert!(dropped.masks.is_empty());
        assert_eq!(losses.masks, 1);
    }

    #[test]
    fn an_xmp_sidecar_imports_through_the_same_reader() {
        let root = temp_root("xmp");
        let p = root.join("look.xmp");
        std::fs::write(
            &p,
            r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
            <rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/" crs:Exposure2012="0.35"/></rdf:RDF></x:xmpmeta>"#,
        )
        .unwrap();
        let r = read_file(&p).unwrap();
        assert!((r.exposure_ev - 0.35).abs() < 1e-6, "{}", r.exposure_ev);
    }
}
