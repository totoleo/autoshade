//! The one door to the UI framework.
//!
//! Production code speaks the app's vocabulary: it imports the framework's
//! names from this module (unqualified, via the crate prelude) and never spells
//! `egui::` itself. Tests still drive the framework directly through the
//! `egui` alias re-exported here. A framework change starts in this module.
//!
//! This first layer isolates the DEPENDENCY and the NAME. The types are still
//! framework aliases: the deeper layer (app-owned value types and a paint
//! vocabulary with conversions) is a separate, larger decision.

/// The framework's own namespace, for the TEST harness (which drives input and
/// rasterises frames directly). Production code uses the names below.
#[cfg(test)]
pub(crate) use eframe::egui;

pub(crate) use eframe::egui::{
    Align,
    Align2,
    Area,
    Button,
    CentralPanel,
    Checkbox,
    CollapsingHeader,
    CollapsingResponse,
    Color32,
    ColorImage,
    ComboBox,
    Context,
    CornerRadius,
    CursorIcon,
    DragValue,
    Event,
    FontData,
    FontDefinitions,
    FontFamily,
    FontId,
    Frame,
    Grid,
    IconData,
    Id,
    Image,
    InnerResponse,
    InputState,
    Key,
    Label,
    LayerId,
    Layout,
    text::LayoutJob,
    Margin,
    Mesh,
    Modifiers,
    Order,
    PointerButton,
    Pos2,
    ProgressBar,
    Rangef,
    Rect,
    epaint::RectShape,
    Response,
    RichText,
    ScrollArea,
    Sense,
    epaint::Shadow,
    Shape,
    SidePanel,
    Slider,
    Stroke,
    StrokeKind,
    Style,
    SystemTheme,
    TextEdit,
    TextFormat,
    TextStyle,
    TextWrapMode,
    TextureHandle,
    TextureId,
    TextureOptions,
    ThemePreference,
    TopBottomPanel,
    Ui,
    Vec2,
    ViewportBuilder,
    ViewportCommand,
    Visuals,
    WidgetText,
    Window,
    collapsing_header,
    ecolor,
    emath,
    lerp,
    load,
    containers::panel,
    pos2,
    vec2,
    viewport,
};

/// Test-only: the harness builds raw frames from this.
#[cfg(test)]
pub(crate) use eframe::egui::RawInput;
