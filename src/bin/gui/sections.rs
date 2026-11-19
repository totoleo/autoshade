//! The develop panel's section table: display order, the control families whose
//! activity lights the section's ●, and the section's OWN draw.
//!
//! A row here is the whole registration: `develop_panel` walks the table and
//! calls `(section.draw)(self, ui, active)` — adding a section is a row plus its
//! draw function in the section's module, never an enum variant and a central
//! `match` in the panel. The table lives in the view layer because a row owns
//! view behaviour; the headless logic layer never sees it.

use crate::*;
use autoshade::advisor::catalogue::{family_is_active, CONTROL_FAMILIES};
use autoshade::recipe::EditRecipe;

/// How a section draws, for the registry contract (the dispatch is the row's
/// own `draw`): a capability form or a custom widget.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SectionKind {
    Form,
    Custom,
}

/// Where a section's draw lives: still on the command context, or already a
/// PanelView method. The table is the single place either is named.
pub(crate) enum SectionDraw {
    App(fn(&mut Commands<'_>, &mut Ui, bool) -> bool),
    Panel(fn(&mut PanelView<'_, '_>, &mut Ui, bool) -> bool),
}

/// One row of the panel.
#[allow(dead_code)] // `id`/`kind` are the registry contract read by the tests; `draw` is the dispatch
pub(crate) struct Section {
    /// Stable identity for the table's tests and diagnostics.
    pub id: &'static str,
    /// The control families the section's ● reads (empty = no dot of its own).
    pub families: &'static [&'static str],
    pub kind: SectionKind,
    /// The section's draw, in one uniform shape so the panel needs no match.
    pub draw: SectionDraw,
}

fn panel_dev_presence(view: &mut PanelView<'_, '_>, ui: &mut Ui, active: bool) -> bool {
    view.dev_presence(ui, active)
}

fn panel_dev_calibration(view: &mut PanelView<'_, '_>, ui: &mut Ui, active: bool) -> bool {
    view.dev_calibration(ui, active)
}

fn panel_dev_effects(view: &mut PanelView<'_, '_>, ui: &mut Ui, active: bool) -> bool {
    view.dev_effects(ui, active)
}

fn panel_dev_hdr(view: &mut PanelView<'_, '_>, ui: &mut Ui, active: bool) -> bool {
    view.dev_hdr(ui, active)
}

fn panel_dev_transform(view: &mut PanelView<'_, '_>, ui: &mut Ui, active: bool) -> bool {
    view.dev_transform(ui, active)
}

fn panel_dev_lens(view: &mut PanelView<'_, '_>, ui: &mut Ui, active: bool) -> bool {
    view.dev_lens(ui, active)
}

fn panel_dev_curves(view: &mut PanelView<'_, '_>, ui: &mut Ui, active: bool) -> bool {
    view.dev_curves(ui, active)
}

fn panel_dev_grading(view: &mut PanelView<'_, '_>, ui: &mut Ui, active: bool) -> bool {
    view.dev_grading(ui, active)
}

fn panel_dev_stack(view: &mut PanelView<'_, '_>, ui: &mut Ui, active: bool) -> bool {
    view.dev_stack(ui, active)
}

fn panel_dev_lens_blur(view: &mut PanelView<'_, '_>, ui: &mut Ui, active: bool) -> bool {
    view.dev_lens_blur(ui, active)
}

fn panel_dev_detail(view: &mut PanelView<'_, '_>, ui: &mut Ui, active: bool) -> bool {
    view.dev_detail(ui, active)
}

fn panel_dev_hsl(view: &mut PanelView<'_, '_>, ui: &mut Ui, active: bool) -> bool {
    view.dev_hsl(ui, active)
}

fn app_dev_versions(view: &mut Commands<'_>, ui: &mut Ui, active: bool) -> bool {
    view.dev_versions(ui, active)
}

fn panel_dev_tone_wb(view: &mut PanelView<'_, '_>, ui: &mut Ui, active: bool) -> bool {
    view.dev_tone_wb(ui, active)
}

fn panel_dev_crop(view: &mut PanelView<'_, '_>, ui: &mut Ui, active: bool) -> bool {
    view.dev_crop(ui, active)
}

fn panel_dev_export(view: &mut PanelView<'_, '_>, ui: &mut Ui, active: bool) -> bool {
    view.dev_export(ui, active)
}

fn panel_dev_masks(view: &mut PanelView<'_, '_>, ui: &mut Ui, active: bool) -> bool {
    view.dev_masks(ui, active)
}

/// The panel's sections, in the order it draws them.
pub(crate) const SECTIONS: &[Section] = &[
    Section {
        id: "tone_wb",
        families: &["tone", "white_balance"],
        kind: SectionKind::Form,
        draw: SectionDraw::Panel(panel_dev_tone_wb),
    },
    Section {
        id: "presence",
        families: &["presence"],
        kind: SectionKind::Form,
        draw: SectionDraw::Panel(panel_dev_presence),
    },
    Section {
        id: "curves",
        families: &["curves", "parametric"],
        kind: SectionKind::Custom,
        draw: SectionDraw::Panel(panel_dev_curves),
    },
    Section {
        id: "hsl",
        families: &["hsl", "black_white", "point_color"],
        kind: SectionKind::Custom,
        draw: SectionDraw::Panel(panel_dev_hsl),
    },
    Section {
        id: "grading",
        families: &["color_grade"],
        kind: SectionKind::Custom,
        draw: SectionDraw::Panel(panel_dev_grading),
    },
    Section {
        id: "calibration",
        families: &["calibration"],
        kind: SectionKind::Form,
        draw: SectionDraw::Panel(panel_dev_calibration),
    },
    Section {
        id: "detail",
        families: &["detail", "detail_effects"],
        kind: SectionKind::Form,
        draw: SectionDraw::Panel(panel_dev_detail),
    },
    Section {
        id: "effects",
        families: &["effects"],
        kind: SectionKind::Form,
        draw: SectionDraw::Panel(panel_dev_effects),
    },
    // Lens Blur returns ACTIONS (its build is a worker verb): the row's draw
    // applies them, and the section has no ● of its own and no family.
    Section {
        id: "lens_blur",
        families: &[],
        kind: SectionKind::Form,
        draw: SectionDraw::Panel(panel_dev_lens_blur),
    },
    Section {
        id: "lens",
        families: &["lens", "lens_effects"],
        kind: SectionKind::Form,
        draw: SectionDraw::Panel(panel_dev_lens),
    },
    Section {
        id: "transform",
        families: &["transform"],
        kind: SectionKind::Form,
        draw: SectionDraw::Panel(panel_dev_transform),
    },
    // Crop/straighten live in the `framing` family; the section derives that ●
    // itself (the same rule as the others).
    Section {
        id: "crop",
        families: &["framing"],
        kind: SectionKind::Form,
        draw: SectionDraw::Panel(panel_dev_crop),
    },
    Section {
        id: "masks",
        families: &[],
        kind: SectionKind::Custom,
        draw: SectionDraw::Panel(panel_dev_masks),
    },
    Section {
        id: "versions",
        families: &[],
        kind: SectionKind::Form,
        draw: SectionDraw::App(app_dev_versions),
    },
    Section {
        id: "stack",
        families: &[],
        kind: SectionKind::Form,
        draw: SectionDraw::Panel(panel_dev_stack),
    },
    Section {
        id: "hdr",
        families: &["hdr"],
        kind: SectionKind::Form,
        draw: SectionDraw::Panel(panel_dev_hdr),
    },
    Section {
        id: "export",
        families: &[],
        kind: SectionKind::Form,
        draw: SectionDraw::Panel(panel_dev_export),
    },
];

/// Is any of `section`'s families active on this recipe (its ● predicate)?
pub(crate) fn section_active(recipe: &EditRecipe, section: &Section) -> bool {
    section.families.iter().any(|name| {
        CONTROL_FAMILIES
            .iter()
            .find(|f| f.name == *name)
            .is_some_and(|f| family_is_active(f, recipe))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_section_family_is_a_registered_control_family() {
        for section in SECTIONS {
            for family in section.families {
                assert!(
                    CONTROL_FAMILIES.iter().any(|f| f.name == *family),
                    "section `{}` names the unknown family `{family}`",
                    section.id,
                );
            }
        }
    }

    #[test]
    fn every_section_id_appears_exactly_once_in_display_order() {
        let mut ids: Vec<&str> = SECTIONS.iter().map(|s| s.id).collect();
        let before = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), before, "a section id is listed twice");
        assert_eq!(SECTIONS.len(), 17, "the display order is the panel's; a new row changes it deliberately");
        // The order is the panel's product contract: pin the ids in it.
        assert_eq!(
            SECTIONS.iter().map(|s| s.id).collect::<Vec<_>>(),
            [
                "tone_wb", "presence", "curves", "hsl", "grading", "calibration", "detail",
                "effects", "lens_blur", "lens", "transform", "crop", "masks", "versions",
                "stack", "hdr", "export",
            ],
        );
    }

    #[test]
    fn the_custom_sections_register_a_custom_widget() {
        let custom: Vec<&str> =
            SECTIONS.iter().filter(|s| s.kind == SectionKind::Custom).map(|s| s.id).collect();
        assert_eq!(
            custom,
            ["curves", "hsl", "grading", "masks"],
            "the custom widgets are the curve editor, the mixer, the wheels and the mask panel",
        );
    }

    #[test]
    fn the_tone_section_lights_on_either_of_its_two_families() {
        let tone = SECTIONS.iter().find(|s| s.id == "tone_wb").unwrap();
        let mut recipe = EditRecipe::default();
        assert!(!section_active(&recipe, tone), "a stock recipe lights nothing");
        recipe.exposure_ev = 0.5;
        assert!(section_active(&recipe, tone), "a tone edit lights the section");
        let mut wb = EditRecipe::default();
        wb.temperature_k = Some(6000.0);
        assert!(section_active(&wb, tone), "a WB edit lights it too");
    }
}
