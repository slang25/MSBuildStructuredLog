//! Node icons.
//!
//! The shape vocabulary is the WPF viewer's (`themes/Generic.xaml`): a small
//! rounded square for most kinds — the colour *is* the type — with a real
//! shape for the few that earn one (a plus and a minus for
//! AddItem/RemoveItem, the NuGet mark for a package, a box-and-wire for a
//! file copy, a page for a project). Squares are `div`s; the shapes are
//! `canvas` paths.
//!
//! A kind resolves to one [`Tone`], and every part of its icon — the
//! hairline, the wash behind it — is derived from that single accent
//! composed over the row background. That is what makes one icon set work in
//! both themes: there is no second palette to keep in sync, and nothing is a
//! fixed pastel that only reads on white.
//!
//! `surface` is that background. The NuGet mark has two holes punched
//! through it, and the only honest way to draw a hole is to paint the
//! background back over it.

use crate::theme::Theme;
use gpui::{
    App, BorderStyle, Bounds, Edges, Hsla, PathBuilder, Pixels, Window, canvas, div, point,
    prelude::*, px, quad, rgb, size,
};

/// The icon box every row reserves, in pixels.
pub const SIZE: f32 = 16.;

/// The chip itself, centred in that box.
const CHIP: f32 = 12.;
const RADIUS: f32 = 3.5;
const HAIRLINE: f32 = 1.;

fn c(hex: u32) -> Hsla {
    rgb(hex).into()
}

// ===================================================================
// Tones
// ===================================================================

/// The semantic colour of a node kind, resolved per theme.
#[derive(Clone, Copy, PartialEq)]
pub enum Tone {
    Success,
    Error,
    Warning,
    Folder,
    Target,
    Task,
    Item,
    Metadata,
    Property,
    Message,
    Server,
    Import,
    NoImport,
    NuGet,
    /// A project's language tint, which `styling.rs` derives per extension.
    Custom(Hsla),
}

impl Tone {
    /// The one colour the tone is. Hues are the GitHub Primer ramps, picked
    /// to stay distinguishable from each other and to hold contrast on
    /// either background.
    pub fn accent(self, theme: &Theme) -> Hsla {
        let (light, dark) = match self {
            Tone::Success => (0x1a7f37, 0x3fb950),
            Tone::Error => (0xcf222e, 0xf85149),
            Tone::Warning => (0x9a6700, 0xd29922),
            Tone::Folder => (0x977405, 0xd0a215),
            Tone::Target => (0x8250df, 0xa371f7),
            Tone::Task => (0x0969da, 0x58a6ff),
            Tone::Item => (0x1f883d, 0x56d364),
            Tone::Metadata => (0x137d84, 0x39c5cf),
            Tone::Property => (0x4a52c4, 0x8b95f6),
            Tone::Message => (0x6e7781, 0x8b949e),
            Tone::Server => (0x6b4bc7, 0x9d8df1),
            Tone::Import => (0xa74c00, 0xe0904f),
            Tone::NoImport => (0xcf222e, 0xf85149),
            Tone::NuGet => (0x00558f, 0x4db8ff),
            Tone::Custom(color) => return color,
        };
        c(if theme.dark { dark } else { light })
    }

    /// The wash inside the hairline. Dark rows need more of it to read as a
    /// fill at all; light rows need less before the icon starts shouting.
    fn wash(self, theme: &Theme) -> Hsla {
        self.accent(theme).opacity(if theme.dark { 0.30 } else { 0.18 })
    }

    fn hairline(self, theme: &Theme) -> Hsla {
        self.accent(theme).opacity(0.9)
    }
}

// ===================================================================
// Icons
// ===================================================================

/// Which end of a file copy the row is about (the bridge's `copyKind`).
#[derive(Clone, Copy, PartialEq)]
pub enum CopyEnd {
    Both,
    Source,
    Destination,
}

#[derive(Clone, Copy, PartialEq)]
pub enum NodeIcon {
    Chip(Tone),
    /// AddItem and task item parameters.
    Plus(Tone),
    /// RemoveItem.
    Minus(Tone),
    /// A NuGet package.
    Package,
    /// A copied file, emphasising one end.
    Copy(CopyEnd, Tone),
    /// A project file, tinted by language. Evaluations get a dashed outline:
    /// the file was read, not built.
    Document { tint: Hsla, evaluation: bool },
}

/// Renders `icon` into a `SIZE`-square box sitting on `surface`.
pub fn render(icon: NodeIcon, theme: &Theme, surface: Hsla) -> gpui::AnyElement {
    let theme = *theme;
    match icon {
        NodeIcon::Chip(tone) => centered(rectangle(tone, &theme, CHIP, CHIP, RADIUS)),
        NodeIcon::Minus(tone) => centered(rectangle(tone, &theme, CHIP, 4., 1.5)),
        NodeIcon::Copy(end, tone) => copy(end, tone.accent(&theme)),
        NodeIcon::Plus(tone) => painted(move |bounds, window| paint_plus(bounds, tone, &theme, window)),
        NodeIcon::Package => {
            let color = Tone::NuGet.accent(&theme);
            painted(move |bounds, window| paint_package(bounds, color, surface, window))
        }
        NodeIcon::Document { tint, evaluation } => {
            painted(move |bounds, window| paint_document(bounds, tint, evaluation, window))
        }
    }
}

fn centered(child: impl IntoElement) -> gpui::AnyElement {
    div()
        .w(px(SIZE))
        .h(px(SIZE))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .child(child)
        .into_any_element()
}

fn painted(paint: impl 'static + FnOnce(Bounds<Pixels>, &mut Window)) -> gpui::AnyElement {
    div()
        .w(px(SIZE))
        .h(px(SIZE))
        .flex_none()
        .child(canvas(|_, _, _| (), move |bounds, _, window: &mut Window, _: &mut App| paint(bounds, window)).size_full())
        .into_any_element()
}

fn rectangle(tone: Tone, theme: &Theme, w: f32, h: f32, radius: f32) -> impl IntoElement {
    div()
        .w(px(w))
        .h(px(h))
        .rounded(px(radius))
        .bg(tone.wash(theme))
        .border(px(HAIRLINE))
        .border_color(tone.hairline(theme))
}

/// Two boxes on a wire; the end the row is about is the one drawn.
fn copy(end: CopyEnd, color: Hsla) -> gpui::AnyElement {
    let (box_x, wire_from, wire_to) = match end {
        CopyEnd::Source => (0., 6., 16.),
        CopyEnd::Destination => (10., 0., 10.),
        CopyEnd::Both => (5., 0., 16.),
    };
    div()
        .w(px(SIZE))
        .h(px(SIZE))
        .flex_none()
        .relative()
        .child(
            div()
                .absolute()
                .top(px(7.5))
                .left(px(wire_from))
                .w(px(wire_to - wire_from))
                .h(px(1.))
                .bg(color.opacity(0.7)),
        )
        .child(div().absolute().top(px(5.)).left(px(box_x)).w(px(6.)).h(px(6.)).rounded(px(1.5)).bg(color))
        .into_any_element()
}

/// The WPF `AddItemGeometry`, authored on a 13x11 grid.
fn paint_plus(bounds: Bounds<Pixels>, tone: Tone, theme: &Theme, window: &mut Window) {
    const POINTS: [(f32, f32); 12] = [
        (2., 4.),
        (5., 4.),
        (5., 1.),
        (8., 1.),
        (8., 4.),
        (11., 4.),
        (11., 7.),
        (8., 7.),
        (8., 10.),
        (5., 10.),
        (5., 7.),
        (2., 7.),
    ];
    let origin = bounds.origin;
    let at = |(x, y): (f32, f32)| point(origin.x + px(x + 1.5), origin.y + px(y + 2.5));
    let points: Vec<_> = POINTS.into_iter().map(at).collect();

    let mut fill = PathBuilder::fill();
    fill.add_polygon(&points, true);
    if let Ok(path) = fill.build() {
        window.paint_path(path, tone.wash(theme));
    }
    let mut stroke = PathBuilder::stroke(px(HAIRLINE));
    stroke.add_polygon(&points, true);
    if let Ok(path) = stroke.build() {
        window.paint_path(path, tone.hairline(theme));
    }
}

/// The NuGet mark (`NuGetGeometry`): a rounded square with two round holes
/// and a detached dot, authored on the same 16-unit grid we render into. A
/// logo stays solid rather than taking the wash — it is a mark, not a tone.
fn paint_package(bounds: Bounds<Pixels>, color: Hsla, surface: Hsla, window: &mut Window) {
    let origin = bounds.origin;
    let disc = |cx: f32, cy: f32, r: f32, color: Hsla, window: &mut Window| {
        let rect = Bounds::new(
            point(origin.x + px(cx - r), origin.y + px(cy - r)),
            size(px(r * 2.), px(r * 2.)),
        );
        window.paint_quad(quad(rect, px(r), color, Edges::all(px(0.)), color, BorderStyle::Solid));
    };

    let body = Bounds::new(point(origin.x + px(4.), origin.y + px(4.)), size(px(11.), px(11.)));
    window.paint_quad(quad(body, px(3.5), color, Edges::all(px(0.)), color, BorderStyle::Solid));
    disc(7., 7., 1.45, surface, window);
    disc(11.48, 11.48, 2.32, surface, window);
    disc(2.5, 2.5, 1.17, color, window);
}

/// A page with a folded corner, tinted by language.
fn paint_document(bounds: Bounds<Pixels>, tint: Hsla, evaluation: bool, window: &mut Window) {
    let origin = bounds.origin;
    let at = |x: f32, y: f32| point(origin.x + px(x), origin.y + px(y));
    let outline = [at(2.5, 1.), at(9., 1.), at(13., 5.), at(13., 15.), at(2.5, 15.)];
    let fold = [at(9., 1.), at(13., 5.), at(9., 5.)];
    let (body, corner) = if evaluation { (0.16, 0.47) } else { (0.40, 0.85) };

    let mut fill = PathBuilder::fill();
    fill.add_polygon(&outline, true);
    if let Ok(path) = fill.build() {
        window.paint_path(path, tint.opacity(body));
    }
    let mut fold_path = PathBuilder::fill();
    fold_path.add_polygon(&fold, true);
    if let Ok(path) = fold_path.build() {
        window.paint_path(path, tint.opacity(corner));
    }
    // An evaluation read the file rather than building it; the dashed
    // outline is the whole difference between the two rows' icons.
    let mut stroke = PathBuilder::stroke(px(HAIRLINE));
    if evaluation {
        stroke = stroke.dash_array(&[px(2.), px(1.6)]);
    }
    stroke.add_polygon(&outline, true);
    if let Ok(path) = stroke.build() {
        window.paint_path(path, tint);
    }
}
