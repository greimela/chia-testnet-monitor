//! The page's two palettes, copied token for token from index.html.

use gpui_kit::{Hsla, rgb};

#[derive(Clone, Copy)]
pub struct Theme {
    pub paper: Hsla,
    pub band: Hsla,
    pub ink: Hsla,
    pub ink2: Hsla,
    pub ink3: Hsla,
    pub rule: Hsla,
    pub faint: Hsla,
    pub v1: Hsla,
    pub v2: Hsla,
    pub yours: Hsla,
    pub warn: Hsla,
    pub crit: Hsla,
}

fn c(hex: u32) -> Hsla {
    rgb(hex).into()
}

impl Theme {
    pub fn new(dark: bool) -> Self {
        if dark {
            Self {
                paper: c(0x101813),
                band: c(0x16211a),
                ink: c(0xe4ebe3),
                ink2: c(0xa7b4ab),
                ink3: c(0x75847a),
                rule: c(0x2a382f),
                faint: c(0x1f2b23),
                v1: c(0x5b86d0),
                v2: c(0x2fa457),
                yours: c(0xcf5f9b),
                warn: c(0xe39a3b),
                crit: c(0xe2625f),
            }
        } else {
            Self {
                paper: c(0xf2f4ef),
                band: c(0xe5eae2),
                ink: c(0x13241a),
                ink2: c(0x4c5b52),
                ink3: c(0x75837a),
                rule: c(0xcfd8cd),
                faint: c(0xdbe2d8),
                v1: c(0x3d6bb3),
                v2: c(0x2e9b4e),
                yours: c(0xb0457f),
                warn: c(0xa85a0c),
                crit: c(0xc23a3a),
            }
        }
    }

    pub fn ver(&self, v: &str) -> Hsla {
        if v == "v2" { self.v2 } else { self.v1 }
    }
}

/// `font-variant-numeric: tabular-nums`: digits all one width, as the page sets almost everywhere.
pub fn tnum() -> gpui_kit::FontFeatures {
    gpui_kit::FontFeatures(std::sync::Arc::new(vec![("tnum".to_string(), 1)]))
}

pub fn with_alpha(mut c: Hsla, a: f32) -> Hsla {
    c.a = a;
    c
}

// Font families: static instances of Archivo cut from the variable font at the
// width/weight pairs the page's CSS asks for (see assets/).
pub const REG: &str = "ArchivoR"; // 400
pub const MED: &str = "ArchivoM"; // 500
pub const SEMI: &str = "ArchivoSB"; // 600
pub const BOLD: &str = "ArchivoB"; // 650
pub const BRAND: &str = "ArchivoC88"; // wdth 88, 750
pub const BIG: &str = "ArchivoC80"; // wdth 80, 720
pub const FACT: &str = "ArchivoC82"; // wdth 82, 680
pub const STAT: &str = "ArchivoC85"; // wdth 85, 600
pub const CD: &str = "ArchivoC66"; // wdth 66, 780
pub const CD_LIGHT: &str = "ArchivoC66L"; // 300
pub const CD_MED: &str = "ArchivoC66M"; // 600
#[cfg(target_os = "macos")]
pub const MONO: &str = "Menlo";
#[cfg(target_os = "windows")]
pub const MONO: &str = "Consolas";
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub const MONO: &str = "DejaVu Sans Mono";

pub const FONT_FILES: &[&[u8]] = &[
    include_bytes!("../assets/fonts/ArchivoR.ttf"),
    include_bytes!("../assets/fonts/ArchivoM.ttf"),
    include_bytes!("../assets/fonts/ArchivoSB.ttf"),
    include_bytes!("../assets/fonts/ArchivoB.ttf"),
    include_bytes!("../assets/fonts/ArchivoC88.ttf"),
    include_bytes!("../assets/fonts/ArchivoC80.ttf"),
    include_bytes!("../assets/fonts/ArchivoC82.ttf"),
    include_bytes!("../assets/fonts/ArchivoC85.ttf"),
    include_bytes!("../assets/fonts/ArchivoC66.ttf"),
    include_bytes!("../assets/fonts/ArchivoC66L.ttf"),
    include_bytes!("../assets/fonts/ArchivoC66M.ttf"),
];
