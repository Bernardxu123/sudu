use iced::{Color, Font};
use std::path::PathBuf;
use std::sync::OnceLock;

#[derive(Clone, Copy)]
pub struct Palette {
    pub bg: Color,
    pub panel: Color,
    pub text: Color,
    pub muted: Color,
    pub accent: Color,
    pub selection: Color,
    pub highlight: Color,
    pub highlight_current: Color,
    pub code_bg: Color,
    pub border: Color,
}

#[derive(Clone)]
pub struct Theme {
    pub name: &'static str,
    pub dark: bool,
    pub pal: Palette,
    pub body: Font,
    pub mono: Font,
    pub size: f32,
    pub line: f32,
    pub width: f32,
}

struct Fonts {
    sans: Font,
    mono: Font,
    ui: Font,
}

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgb8(r, g, b)
}

fn font_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(w) = std::env::var_os("WINDIR") {
        out.push(PathBuf::from(w).join("Fonts"));
    }
    if let Some(l) = std::env::var_os("LOCALAPPDATA") {
        out.push(
            PathBuf::from(l)
                .join("Microsoft")
                .join("Windows")
                .join("Fonts"),
        );
    }
    out
}

fn has_font(files: &[&str]) -> bool {
    let dirs = font_dirs();
    files
        .iter()
        .any(|name| dirs.iter().any(|dir| dir.join(name).exists()))
}

fn pick(files: &[&str], family: &'static str, fallback: &'static str) -> Font {
    if has_font(files) {
        Font::with_name(family)
    } else {
        Font::with_name(fallback)
    }
}

fn fonts() -> &'static Fonts {
    static FONTS: OnceLock<Fonts> = OnceLock::new();
    FONTS.get_or_init(|| {
        let sans = pick(&["msyh.ttc"], "Microsoft YaHei UI", "Segoe UI");
        let ui = pick(&["msyh.ttc"], "Microsoft YaHei UI", "Segoe UI");
        let mono = if has_font(&["consola.ttf"]) {
            Font::with_name("Consolas")
        } else if has_font(&["CascadiaMono.ttf"]) {
            Font::with_name("Cascadia Mono")
        } else {
            Font::with_name("Courier New")
        };
        Fonts { sans, mono, ui }
    })
}

pub fn ui_font() -> Font {
    fonts().ui
}

pub fn all() -> [Theme; 4] {
    let f = fonts();
    [
        Theme {
            name: "白",
            dark: false,
            pal: Palette {
                bg: rgb(0xFF, 0xFF, 0xFF),
                panel: rgb(0xF5, 0xF6, 0xF8),
                text: rgb(0x1B, 0x1D, 0x21),
                muted: rgb(0x6A, 0x70, 0x78),
                accent: rgb(0x2F, 0x6F, 0x8F),
                selection: rgb(0xCF, 0xE8, 0xF3),
                highlight: rgb(0xEA, 0xF3, 0xF8),
                highlight_current: rgb(0xC9, 0xE4, 0xF2),
                code_bg: rgb(0xF2, 0xF3, 0xF5),
                border: rgb(0xE3, 0xE6, 0xEA),
            },
            body: f.sans,
            mono: f.mono,
            size: 17.0,
            line: 1.85,
            width: 760.0,
        },
        Theme {
            name: "纸",
            dark: false,
            pal: Palette {
                bg: rgb(0xFA, 0xF7, 0xEF),
                panel: rgb(0xF3, 0xED, 0xE1),
                text: rgb(0x2B, 0x26, 0x20),
                muted: rgb(0x7A, 0x6F, 0x60),
                accent: rgb(0xA4, 0x55, 0x2F),
                selection: rgb(0xEB, 0xD9, 0xC9),
                highlight: rgb(0xF1, 0xE6, 0xD6),
                highlight_current: rgb(0xE2, 0xC8, 0xA6),
                code_bg: rgb(0xF1, 0xE9, 0xDA),
                border: rgb(0xE2, 0xD7, 0xC5),
            },
            body: f.sans,
            mono: f.mono,
            size: 18.0,
            line: 1.95,
            width: 740.0,
        },
        Theme {
            name: "夜",
            dark: true,
            pal: Palette {
                bg: rgb(0x15, 0x17, 0x1C),
                panel: rgb(0x1B, 0x1E, 0x25),
                text: rgb(0xD9, 0xDC, 0xE2),
                muted: rgb(0x8B, 0x92, 0x9E),
                accent: rgb(0xD0, 0x8A, 0x5A),
                selection: rgb(0x2E, 0x3A, 0x46),
                highlight: rgb(0x26, 0x31, 0x3A),
                highlight_current: rgb(0x3C, 0x55, 0x63),
                code_bg: rgb(0x1E, 0x22, 0x2A),
                border: rgb(0x2A, 0x2F, 0x38),
            },
            body: f.sans,
            mono: f.mono,
            size: 17.0,
            line: 1.9,
            width: 760.0,
        },
        Theme {
            name: "米黄",
            dark: false,
            pal: Palette {
                bg: rgb(0xF4, 0xEB, 0xD8),
                panel: rgb(0xED, 0xE2, 0xCA),
                text: rgb(0x3A, 0x32, 0x26),
                muted: rgb(0x7D, 0x6F, 0x58),
                accent: rgb(0x8A, 0x6A, 0x3F),
                selection: rgb(0xE3, 0xD3, 0xB4),
                highlight: rgb(0xEB, 0xDD, 0xC6),
                highlight_current: rgb(0xDC, 0xC1, 0x98),
                code_bg: rgb(0xED, 0xE1, 0xC9),
                border: rgb(0xDC, 0xCB, 0xA8),
            },
            body: f.sans,
            mono: f.mono,
            size: 18.0,
            line: 1.95,
            width: 740.0,
        },
    ]
}

pub fn base_theme(theme: &Theme) -> iced::Theme {
    if theme.dark {
        iced::Theme::Dark
    } else {
        iced::Theme::Light
    }
}
