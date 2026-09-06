//! Palette that follows the window appearance, aimed at the look of an
//! AppKit source-list window: vibrant grey sidebar, white/near-black
//! content, system-accent selection when focused, grey when not.

use gpui::{Global, Hsla, WindowAppearance, rgb, rgba};

/// gpui_web embeds Lilex; the native build uses the system monospace.
pub const MONO: &str = if cfg!(target_family = "wasm") { "Lilex" } else { "Menlo" };

#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub dark: bool,
    pub window_background: Hsla,
    pub sidebar_background: Hsla,
    pub titlebar_background: Hsla,
    pub content_background: Hsla,
    pub border: Hsla,
    pub guide: Hsla,
    pub text: Hsla,
    pub text_secondary: Hsla,
    pub text_tertiary: Hsla,
    pub text_on_accent: Hsla,
    pub accent: Hsla,
    pub selection: Hsla,
    pub selection_inactive: Hsla,
    pub hover: Hsla,
    pub field_background: Hsla,
    pub field_border: Hsla,
    pub badge_background: Hsla,
    pub badge_text: Hsla,
    pub chip_background: Hsla,
    pub chip_text: Hsla,
    pub highlight_background: Hsla,
    pub highlight_text: Hsla,
    pub success: Hsla,
    pub warning: Hsla,
    pub error: Hsla,
    pub link: Hsla,
}

impl Global for Theme {}

impl Theme {
    pub fn for_appearance(appearance: WindowAppearance) -> Theme {
        match appearance {
            WindowAppearance::Dark | WindowAppearance::VibrantDark => Theme::dark(),
            _ => Theme::light(),
        }
    }

    pub fn light() -> Theme {
        Theme {
            dark: false,
            window_background: rgb(0xffffff).into(),
            sidebar_background: rgb(0xf3f3f3).into(),
            titlebar_background: rgb(0xf6f6f6).into(),
            content_background: rgb(0xffffff).into(),
            border: rgb(0xd9d9d9).into(),
            guide: rgba(0x00000014).into(),
            text: rgb(0x1d1d1f).into(),
            text_secondary: rgb(0x6e6e73).into(),
            text_tertiary: rgb(0xa1a1a6).into(),
            text_on_accent: rgb(0xffffff).into(),
            accent: rgb(0x0a60ff).into(),
            selection: rgb(0x0a60ff).into(),
            selection_inactive: rgb(0xdcdcdc).into(),
            hover: rgba(0x0000000a).into(),
            field_background: rgb(0xffffff).into(),
            field_border: rgb(0xc9c9cc).into(),
            badge_background: rgb(0xe3ecff).into(),
            badge_text: rgb(0x1e4fd6).into(),
            chip_background: rgb(0xffebcd).into(),
            chip_text: rgb(0x7a4a00).into(),
            highlight_background: rgb(0xfff0a8).into(),
            highlight_text: rgb(0x1d1d1f).into(),
            success: rgb(0x1f8a3b).into(),
            warning: rgb(0xb8860b).into(),
            error: rgb(0xd0342c).into(),
            link: rgb(0x0a60ff).into(),
        }
    }

    pub fn dark() -> Theme {
        Theme {
            dark: true,
            window_background: rgb(0x1e1e1e).into(),
            sidebar_background: rgb(0x262626).into(),
            titlebar_background: rgb(0x2a2a2a).into(),
            content_background: rgb(0x1e1e1e).into(),
            border: rgb(0x000000).into(),
            guide: rgba(0xffffff14).into(),
            text: rgb(0xe6e6e6).into(),
            text_secondary: rgb(0x9a9a9f).into(),
            text_tertiary: rgb(0x6a6a70).into(),
            text_on_accent: rgb(0xffffff).into(),
            accent: rgb(0x3d8bff).into(),
            selection: rgb(0x0a5fd6).into(),
            selection_inactive: rgb(0x3a3a3c).into(),
            hover: rgba(0xffffff0a).into(),
            field_background: rgb(0x1f1f1f).into(),
            field_border: rgb(0x3d3d3d).into(),
            badge_background: rgb(0x1f2f4d).into(),
            badge_text: rgb(0x8cb4ff).into(),
            chip_background: rgb(0x474138).into(),
            chip_text: rgb(0xf0c987).into(),
            highlight_background: rgb(0x5a4a10).into(),
            highlight_text: rgb(0xfff2b0).into(),
            success: rgb(0x3fb950).into(),
            warning: rgb(0xe3b341).into(),
            error: rgb(0xf85149).into(),
            link: rgb(0x6ea8ff).into(),
        }
    }
}
