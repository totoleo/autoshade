//! The slider vocabulary the develop forms and the control registry draw
//! with: the shared track (`slider_impl`), the plain/fine/log/pct faces, the
//! hinted tooltip, and the Lightroom companion's explicit-zero encoding.

use crate::*;

/// One labelled slider; double-click resets to `default` (the Lightroom
/// gesture), hover + ↑/↓ nudges by a domain-appropriate step (Shift ×10 —
/// LR's arrow grammar; ←/→ stay library navigation). Returns true if the
/// value changed this frame. Callers pass an already-translated `label`.
/// Whole-number domains (range ≥ 20: the ±100 family, 0..150, hue °) snap
/// to integers while dragging, like Lightroom — the web UI already had to
/// work around raw floats ("13.4849996…" overflowing its value box).
pub(crate) fn slider(
    ui: &mut egui::Ui,
    lang: Lang,
    label: &str,
    value: &mut f32,
    min: f32,
    max: f32,
    default: f32,
) -> bool {
    slider_hinted(ui, lang, label, value, min, max, default, "")
}

/// [`slider`] plus ONE disclosure line at the head of its tooltip — for a
/// control whose value is SHARED with another panel, where the tooltip is
/// the only place that fact can be told. (`slider` returns `bool`, not a
/// `Response`, so a caller cannot chain its own `on_hover_text`; a second
/// tooltip on a wrapper rect would just stack two bubbles over one
/// widget.) The brush radius is the case: one number behind the mask
/// brush AND Fill / Heal / Stamp.
#[allow(clippy::too_many_arguments)] // `slider` + the one extra tooltip line
pub(crate) fn slider_hinted(
    ui: &mut egui::Ui,
    lang: Lang,
    label: &str,
    value: &mut f32,
    min: f32,
    max: f32,
    default: f32,
    hint: &str,
) -> bool {
    let feel =
        if max - min >= 20.0 { SliderFeel::Int } else { SliderFeel::Frac };
    slider_impl(ui, lang, label, value, min, max, default, feel, hint)
}

/// A COMPANION control's slider (`recipe::LR_COMPANION_DEFAULTS`, v1.5.0):
/// it SHOWS what the engine renders — Lightroom's own default while the
/// recipe holds no value — and RETURNS the value to write; the registry's
/// `Companion` implementation restores the explicit-zero encoding (a drag
/// to 0 is a real 0 rather than "absent", and landing on the default
/// returns the control to absent so the sidecar keeps Lightroom's own
/// value). None = the frame changed nothing.
#[allow(clippy::too_many_arguments)] // `slider_impl`'s track + the recipe and the control name
pub(crate) fn companion_slider_edit(
    ui: &mut egui::Ui,
    lang: Lang,
    label: &str,
    recipe: &autoshade::recipe::EditRecipe,
    name: &str,
    min: f32,
    max: f32,
    feel: SliderFeel,
) -> Option<f32> {
    let default = autoshade::recipe::LR_COMPANION_DEFAULTS
        .iter()
        .find(|(n, _)| *n == name)
        .map_or(0.0, |(_, d)| *d);
    let mut shown = recipe.resolved(name);
    slider_impl(ui, lang, label, &mut shown, min, max, default, feel, "").then_some(shown)
}

/// The Sharpening amount's slider (v1.6.0): a COMPANION whose default is
/// Lightroom's own FOR THE KIND of source — 40 on a RAW negative, none on
/// a baked raster — so `raw_source` (`AutoShadeApp::source_is_raw`) picks
/// the default it shows, resets to and returns to absent on. The value
/// shown is the one the engine renders (`EditRecipe::capture_sharpening`);
/// a drag to 0 on a RAW is a real 0, stated in the sidecar.
pub(crate) fn sharpening_slider(
    ui: &mut egui::Ui,
    lang: Lang,
    label: &str,
    recipe: &mut EditRecipe,
    raw_source: bool,
) -> bool {
    let default = if raw_source { autoshade::recipe::LR_RAW_SHARPENING } else { 0.0 };
    let mut shown = recipe.capture_sharpening(raw_source);
    let changed = slider_impl(ui, lang, label, &mut shown, 0.0, 150.0, default, SliderFeel::Int, "");
    if changed {
        if shown == default {
            recipe.clear_resolved("sharpening");
        } else {
            recipe.set_resolved("sharpening", shown);
        }
    }
    changed
}

/// A 0..=1-stored fraction shown on Lightroom's 0..100 track (Amount,
/// feathers, range tolerance): the panel used to mix "Amount 0.65" with
/// "Shadows 40" in one column, which read as two unit systems. Storage
/// stays the fraction (the XMP contract); only the display scales. The
/// ×100 track crosses the ≥ 20 width rule above, so these snap to whole
/// numbers exactly like LR's.
pub(crate) fn slider_pct(
    ui: &mut egui::Ui,
    lang: Lang,
    label: &str,
    value: &mut f32,
    max: f32,
    default: f32,
) -> bool {
    slider_pct_hinted(ui, lang, label, value, max, default, "")
}

/// [`slider_pct`] plus ONE disclosure line at the head of its tooltip — the
/// percent-track twin of [`slider_hinted`], same reason (the helpers return
/// `bool`, so a caller cannot chain its own `on_hover_text`). For a fraction
/// slider whose meaning cannot be read off its label: the AI 「Style」
/// strength, which has to say what it leans on and that 0 means "ignore my
/// habits" (R22 #16 — it was a bare `egui::Slider` carrying that sentence
/// itself, which is exactly why it had no reset gesture and no ↑/↓ nudge).
pub(crate) fn slider_pct_hinted(
    ui: &mut egui::Ui,
    lang: Lang,
    label: &str,
    value: &mut f32,
    max: f32,
    default: f32,
    hint: &str,
) -> bool {
    let mut disp = *value * 100.0;
    let changed = slider_hinted(
        ui,
        lang,
        label,
        &mut disp,
        0.0,
        max * 100.0,
        default * 100.0,
        hint,
    );
    if changed {
        *value = disp / 100.0;
    }
    changed
}

/// Sub-unit precision variant (Straighten °): the whole-number snap of the
/// wide-range class would destroy 0.1° levelling on a ±45 track.
pub(crate) fn slider_fine(
    ui: &mut egui::Ui,
    lang: Lang,
    label: &str,
    value: &mut f32,
    min: f32,
    max: f32,
    default: f32,
) -> bool {
    slider_impl(ui, lang, label, value, min, max, default, SliderFeel::Fine, "")
}

/// Log-scaled variant for values whose useful band is a small fraction of
/// the range — Temp (K): 2000–50000 linear puts 4000–8000 K (where nearly
/// every photo lives) on ~40 px of a 320 px track, making a routine 100 K
/// nudge sub-pixel.
pub(crate) fn slider_log(
    ui: &mut egui::Ui,
    lang: Lang,
    label: &str,
    value: &mut f32,
    min: f32,
    max: f32,
    default: f32,
) -> bool {
    slider_impl(ui, lang, label, value, min, max, default, SliderFeel::LogK, "")
}

#[allow(clippy::too_many_arguments)] // private impl detail shared by four thin public shapes
pub(crate) fn slider_impl(
    ui: &mut egui::Ui,
    lang: Lang,
    label: &str,
    value: &mut f32,
    min: f32,
    max: f32,
    default: f32,
    feel: SliderFeel,
    // Prepended to the shared tooltip; "" for every ordinary slider. See
    // `slider_hinted` — the ONE tooltip is the point, not two stacked.
    hint: &str,
) -> bool {
    let range = max - min;
    // (drag snap, ↑/↓ nudge, shown decimals) per feel class. Frac's snap
    // matches its 2 shown decimals exactly — egui's fixed_decimals ALSO
    // rounds the stored value (slider.rs), so a finer snap would silently
    // lose to the display rounding. The Frac NUDGE is floored at that
    // same 0.01 grid: a finer step (range < 1) rounds back onto the
    // current value below and the `next != *value` guard drops it —
    // ArrowDown on the 0–0.5 Feather was a permanent no-op (D13).
    // EV: 0.01-drag / 0.1-arrow. LogK's
    // nudge is ~1% of the current Kelvin (≈55 K at 5500) — a fixed step
    // would be sub-pixel at one end of a log track and huge at the other.
    let (snap, nudge, decimals) = match feel {
        SliderFeel::Int => (1.0, 1.0, 0usize),
        SliderFeel::Fine => (0.1, 0.1, 1),
        SliderFeel::Frac => (0.01, (range / 100.0).max(0.01), 2),
        SliderFeel::LogK => (1.0, ((*value).abs() * 0.01).max(1.0).round(), 0),
    };
    let grammar = tr(lang, K_DEVELOP_DOUBLE_CLICK_RIGHT_CLICK_RESETS_HOVER_NUDGES_SHI);
    let tip = if hint.is_empty() {
        grammar.to_string()
    } else {
        format!("{hint}\n{grammar}")
    };
    // The value box is egui's DragValue: a button that GROWS with its
    // text, so its width is budgeted at the widest text the track can
    // show (both ends, at the shown decimals) plus the button's padding —
    // the label then truncates into what is left, and a row that fits at
    // its default still fits at either end of a drag (the 2026-09-15
    // 「100」 that ran the panel 4 px wide every frame).
    let value_w = {
        let style = ui.style().drag_value_text_style.resolve(ui.style());
        let widest = ui.fonts_mut(|f| {
            [min, max]
                .into_iter()
                .map(|v| f.layout_no_wrap(format!("{v:.decimals$}"), style.clone(), egui::Color32::WHITE).size().x)
                .fold(0.0f32, f32::max)
        });
        (widest + 2.0 * ui.spacing().button_padding.x).max(ui.spacing().interact_size.x)
    };
    // Lightroom / RapidRaw's slider (2026-10-13, user: 「UI 整体还是不够
    // 现代化」): the name on the left and the number on the right of one
    // line, a thin rail the full width of the column under them. egui's
    // stock row — rail, framed number, name trailing on the right — put
    // the eye on the number box and gave every rail a different length.
    // Both lines take exactly the column's width, so no label length can
    // widen the panel (R38's runaway came from a fixed rail beside one).
    // A vertical child, not a scope: a caller that lays the slider in a
    // row (the Style dial and its warning) gets the two lines as one item
    // of that row instead of the rail continuing the row.
    let scope = ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.spacing_mut().interact_size.y = SLIDER_LINE_H;
        let full = ui.available_width();
        let head = ui.horizontal(|ui| {
            let name_w = (full - value_w - ui.spacing().item_spacing.x).max(0.0);
            let name = ui
                .allocate_ui_with_layout(
                    egui::vec2(name_w, SLIDER_LINE_H),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.set_min_width(name_w);
                        ui.add(
                            egui::Label::new(label)
                                .truncate()
                                .sense(egui::Sense::click()),
                        )
                    },
                )
                .inner;
            // The number reads as text until it is touched: no frame at
            // rest, egui's own frame on hover / drag / typing.
            let number = ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let w = &mut ui.visuals_mut().widgets.inactive;
                w.weak_bg_fill = egui::Color32::TRANSPARENT;
                w.bg_stroke = egui::Stroke::NONE;
                // Hug the text so the digits end where the rail ends.
                ui.spacing_mut().interact_size.x = 0.0;
                // …and keep the line one text high: the button's stock
                // 5 px vertical padding made it 28 px, pushing the rail
                // a whole row away from its name.
                ui.spacing_mut().button_padding = egui::vec2(SPACE_XS, 0.0);
                let speed = if matches!(feel, SliderFeel::LogK) { (*value).abs() * 0.002 } else { range / 300.0 };
                ui.add(
                    egui::DragValue::new(value)
                        .range(min..=max)
                        .speed(f64::from(speed).max(snap * 0.5))
                        .fixed_decimals(decimals),
                )
            });
            (name, number.inner)
        });
        let (name, number) = head.inner;
        ui.spacing_mut().slider_width = full;
        let rail = ui.add(
            egui::Slider::new(value, min..=max)
                .logarithmic(matches!(feel, SliderFeel::LogK))
                .step_by(snap)
                .fixed_decimals(decimals)
                .show_value(false),
        );
        paint_slider_knob(ui, &rail, *value, min, max, matches!(feel, SliderFeel::LogK));
        (name, number, rail)
    });
    let (name, number, rail) = scope.inner;
    let name = name.on_hover_text(&tip);
    let rail = rail.on_hover_text(&tip);
    // Double-click (the Lightroom gesture) or right-click resets, on the
    // rail or on the name — LR resets from either.
    if [&name, &rail].iter().any(|r| r.double_clicked() || r.secondary_clicked()) && *value != default {
        *value = default;
        return true;
    }
    let mut changed = rail.changed() || number.changed();
    // The nudge is gated on "no widget owns the keyboard", so arrows typed
    // into a text field can never double-apply here.
    if (name.hovered() || rail.hovered() || number.hovered()) && ui.memory(|m| m.focused().is_none()) {
        let shift = ui.input(|i| i.modifiers.shift);
        let (up, down) = ui.input_mut(|i| {
            (
                Chord::ARROW_UP.consume(i)
                    || Chord::ARROW_UP.with_shift().consume(i),
                Chord::ARROW_DOWN.consume(i)
                    || Chord::ARROW_DOWN.with_shift().consume(i),
            )
        });
        if up != down {
            let step = nudge * if shift { 10.0 } else { 1.0 };
            let next = crate::logic::slider::nudged(*value, up, step, decimals, min, max);
            if next != *value {
                *value = next;
                changed = true;
            }
        }
    }
    changed
}
