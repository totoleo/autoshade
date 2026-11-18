//! The window's keyboard grammar: every chord the app listens for, named once.
//!
//! The egui `Key`/`Modifiers` mapping is confined to this file, so a handler
//! reads `Chord::ESCAPE.consume(i)` and the framework type never reaches the
//! grammar. Consumption is exact-match, like `InputState::consume_key`: a
//! chord with no modifier does not match the shifted key.

use crate::*;

/// One key the window binds. Only bound keys get a variant; `egui` below is
/// the one place that knows what the framework calls them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Key {
    Escape,
    Enter,
    F1,
    Question,
    O,
    E,
    S,
    Z,
    Y,
    J,
    R,
    W,
    Q,
    K,
    M,
    B,
    Backslash,
    Tab,
    OpenBracket,
    CloseBracket,
    Plus,
    Equals,
    Minus,
    Num0,
    Num1,
    ArrowRight,
    ArrowLeft,
    ArrowUp,
    ArrowDown,
}

/// The modifier set a chord requires, as the app speaks it. `Primary` is the
/// platform's main modifier (Ctrl on Windows/Linux, Command on macOS) — the
/// same one egui calls `COMMAND`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mods {
    None,
    Shift,
    Primary,
    PrimaryShift,
}

/// A keyboard chord the window listens for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Chord {
    mods: Mods,
    key: Key,
}

impl Chord {
    const fn new(mods: Mods, key: Key) -> Self {
        Self { mods, key }
    }

    /// The Shift variant of this chord — for keys whose shifted form has its
    /// own binding (the arrows, `?`, `M`, `+`).
    pub(crate) const fn with_shift(self) -> Self {
        Self { mods: Mods::Shift, key: self.key }
    }

    /// Consume this chord from the frame's input, if it is a fresh press.
    pub(crate) fn consume(self, i: &mut egui::InputState) -> bool {
        i.consume_key(self.mods.egui(), self.key.egui())
    }

    /// Whether the chord's key is held down right now (the modifiers are not
    /// part of a held-key test).
    pub(crate) fn held(self, i: &egui::InputState) -> bool {
        i.key_down(self.key.egui())
    }

    /// Esc: dismiss the topmost transient (and never mutate what is being
    /// dismissed).
    pub(crate) const ESCAPE: Self = Self::new(Mods::None, Key::Escape);
    /// Enter: the primary of a focused layer (commit crop, confirm a prompt).
    pub(crate) const ENTER: Self = Self::new(Mods::None, Key::Enter);
    /// F1: the cheat sheet.
    pub(crate) const F1: Self = Self::new(Mods::None, Key::F1);
    /// `?` (unshifted on some layouts, from `Shift+/` on most).
    pub(crate) const QUESTION: Self = Self::new(Mods::None, Key::Question);
    /// `?` via `Shift+/`.
    pub(crate) const SHIFT_QUESTION: Self = Self::new(Mods::Shift, Key::Question);
    /// Primary+O: open a photo file.
    pub(crate) const CTRL_O: Self = Self::new(Mods::Primary, Key::O);
    /// Primary+E: export (historical alias of `CTRL_SHIFT_E`).
    pub(crate) const CTRL_E: Self = Self::new(Mods::Primary, Key::E);
    /// Primary+Shift+E: export, the editor's own key.
    pub(crate) const CTRL_SHIFT_E: Self = Self::new(Mods::PrimaryShift, Key::E);
    /// Primary+S: save the XMP.
    pub(crate) const CTRL_S: Self = Self::new(Mods::Primary, Key::S);
    /// Primary+Z: undo.
    pub(crate) const CTRL_Z: Self = Self::new(Mods::Primary, Key::Z);
    /// Primary+Shift+Z: redo (the second of the two spellings).
    pub(crate) const CTRL_SHIFT_Z: Self = Self::new(Mods::PrimaryShift, Key::Z);
    /// Primary+Y: redo (the Windows spelling).
    pub(crate) const CTRL_Y: Self = Self::new(Mods::Primary, Key::Y);
    /// O: the overlay / crop-grid toggle (no Primary).
    pub(crate) const O: Self = Self::new(Mods::None, Key::O);
    /// J: the clipping preview.
    pub(crate) const J: Self = Self::new(Mods::None, Key::J);
    /// R: crop.
    pub(crate) const R: Self = Self::new(Mods::None, Key::R);
    /// W: white balance.
    pub(crate) const W: Self = Self::new(Mods::None, Key::W);
    /// Q: heal.
    pub(crate) const Q: Self = Self::new(Mods::None, Key::Q);
    /// K: brush.
    pub(crate) const K: Self = Self::new(Mods::None, Key::K);
    /// M: linear gradient.
    pub(crate) const M: Self = Self::new(Mods::None, Key::M);
    /// Shift+M: radial gradient.
    pub(crate) const SHIFT_M: Self = Self::new(Mods::Shift, Key::M);
    /// B: the compare key held down (before/after peek).
    pub(crate) const B: Self = Self::new(Mods::None, Key::B);
    /// `\`: before/after.
    pub(crate) const BACKSLASH: Self = Self::new(Mods::None, Key::Backslash);
    /// Y: compare.
    pub(crate) const Y: Self = Self::new(Mods::None, Key::Y);
    /// Tab: hide/show the side panels.
    pub(crate) const TAB: Self = Self::new(Mods::None, Key::Tab);
    /// `[`: brush size down.
    pub(crate) const OPEN_BRACKET: Self = Self::new(Mods::None, Key::OpenBracket);
    /// `]`: brush size up.
    pub(crate) const CLOSE_BRACKET: Self = Self::new(Mods::None, Key::CloseBracket);
    /// `+` on the main row.
    pub(crate) const PLUS: Self = Self::new(Mods::None, Key::Plus);
    /// Shift+`: zoom in on layouts where `+` needs Shift.
    pub(crate) const SHIFT_PLUS: Self = Self::new(Mods::Shift, Key::Plus);
    /// `=` (unshifted `+` on ANSI): zoom in.
    pub(crate) const EQUALS: Self = Self::new(Mods::None, Key::Equals);
    /// `-`: zoom out.
    pub(crate) const MINUS: Self = Self::new(Mods::None, Key::Minus);
    /// 0: fit.
    pub(crate) const NUM0: Self = Self::new(Mods::None, Key::Num0);
    /// 1: 1:1.
    pub(crate) const NUM1: Self = Self::new(Mods::None, Key::Num1);
    /// →: next photo (crop armed: nudge the box).
    pub(crate) const ARROW_RIGHT: Self = Self::new(Mods::None, Key::ArrowRight);
    /// ←: previous photo (crop armed: nudge the box).
    pub(crate) const ARROW_LEFT: Self = Self::new(Mods::None, Key::ArrowLeft);
    /// ↑: the vertical twin of ←.
    pub(crate) const ARROW_UP: Self = Self::new(Mods::None, Key::ArrowUp);
    /// ↓: the vertical twin of →.
    pub(crate) const ARROW_DOWN: Self = Self::new(Mods::None, Key::ArrowDown);
}

impl Key {
    fn egui(self) -> egui::Key {
        match self {
            Key::Escape => egui::Key::Escape,
            Key::Enter => egui::Key::Enter,
            Key::F1 => egui::Key::F1,
            Key::Question => egui::Key::Questionmark,
            Key::O => egui::Key::O,
            Key::E => egui::Key::E,
            Key::S => egui::Key::S,
            Key::Z => egui::Key::Z,
            Key::Y => egui::Key::Y,
            Key::J => egui::Key::J,
            Key::R => egui::Key::R,
            Key::W => egui::Key::W,
            Key::Q => egui::Key::Q,
            Key::K => egui::Key::K,
            Key::M => egui::Key::M,
            Key::B => egui::Key::B,
            Key::Backslash => egui::Key::Backslash,
            Key::Tab => egui::Key::Tab,
            Key::OpenBracket => egui::Key::OpenBracket,
            Key::CloseBracket => egui::Key::CloseBracket,
            Key::Plus => egui::Key::Plus,
            Key::Equals => egui::Key::Equals,
            Key::Minus => egui::Key::Minus,
            Key::Num0 => egui::Key::Num0,
            Key::Num1 => egui::Key::Num1,
            Key::ArrowRight => egui::Key::ArrowRight,
            Key::ArrowLeft => egui::Key::ArrowLeft,
            Key::ArrowUp => egui::Key::ArrowUp,
            Key::ArrowDown => egui::Key::ArrowDown,
        }
    }
}

impl Mods {
    fn egui(self) -> egui::Modifiers {
        match self {
            Mods::None => egui::Modifiers::NONE,
            Mods::Shift => egui::Modifiers::SHIFT,
            Mods::Primary => egui::Modifiers::COMMAND,
            Mods::PrimaryShift => egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
        }
    }
}
