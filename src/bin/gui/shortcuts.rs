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
    pub(crate) fn consume(self, i: &mut InputState) -> bool {
        i.consume_key(self.mods.egui(), self.key.egui())
    }

    /// Whether the chord's key is held down right now (the modifiers are not
    /// part of a held-key test).
    pub(crate) fn held(self, i: &InputState) -> bool {
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
    fn egui(self) -> view::Key {
        match self {
            Key::Escape => view::Key::Escape,
            Key::Enter => view::Key::Enter,
            Key::F1 => view::Key::F1,
            Key::Question => view::Key::Questionmark,
            Key::O => view::Key::O,
            Key::E => view::Key::E,
            Key::S => view::Key::S,
            Key::Z => view::Key::Z,
            Key::Y => view::Key::Y,
            Key::J => view::Key::J,
            Key::R => view::Key::R,
            Key::W => view::Key::W,
            Key::Q => view::Key::Q,
            Key::K => view::Key::K,
            Key::M => view::Key::M,
            Key::B => view::Key::B,
            Key::Backslash => view::Key::Backslash,
            Key::Tab => view::Key::Tab,
            Key::OpenBracket => view::Key::OpenBracket,
            Key::CloseBracket => view::Key::CloseBracket,
            Key::Plus => view::Key::Plus,
            Key::Equals => view::Key::Equals,
            Key::Minus => view::Key::Minus,
            Key::Num0 => view::Key::Num0,
            Key::Num1 => view::Key::Num1,
            Key::ArrowRight => view::Key::ArrowRight,
            Key::ArrowLeft => view::Key::ArrowLeft,
            Key::ArrowUp => view::Key::ArrowUp,
            Key::ArrowDown => view::Key::ArrowDown,
        }
    }
}

impl Mods {
    fn egui(self) -> Modifiers {
        match self {
            Mods::None => Modifiers::NONE,
            Mods::Shift => Modifiers::SHIFT,
            Mods::Primary => Modifiers::COMMAND,
            Mods::PrimaryShift => Modifiers::COMMAND | Modifiers::SHIFT,
        }
    }
}
