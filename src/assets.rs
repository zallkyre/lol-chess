use egui::{ColorImage, Context, TextureHandle, TextureOptions};
use std::collections::HashMap;
use std::path::PathBuf;

pub fn root() -> PathBuf {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|p| p.to_path_buf()));
    if let Some(mut p) = exe_dir.clone() {
        for _ in 0..4 {
            if p.join("lichess").exists() || p.join("engine").exists() {
                return p;
            }
            match p.parent() {
                Some(parent) => p = parent.to_path_buf(),
                None => break,
            }
        }
    }
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if manifest.exists() {
        return manifest;
    }
    exe_dir.unwrap_or_else(|| PathBuf::from("."))
}

pub const PIECE_SETS: &[&str] = &["cburnett", "merida", "alpha", "staunty", "fantasy", "chessnut"];

pub struct Theme {
    pub name: &'static str,
    pub light: egui::Color32,
    pub dark: egui::Color32,
}

const fn c(hex: u32) -> egui::Color32 {
    egui::Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

pub const THEMES: &[Theme] = &[
    Theme { name: "brown", light: c(0xf0d9b5), dark: c(0xb58863) },
    Theme { name: "blue", light: c(0xdee3e6), dark: c(0x8ca2ad) },
    Theme { name: "green", light: c(0xeeeed2), dark: c(0x769656) },
    Theme { name: "grey", light: c(0xd8d8d8), dark: c(0x9e9e9e) },
    Theme { name: "purple", light: c(0xe8e5f0), dark: c(0x9d8dae) },
    Theme { name: "olive", light: c(0xebe6d6), dark: c(0x9a9a6c) },
    Theme { name: "wood", light: c(0xd9c3a5), dark: c(0xa97f58) },
];

fn unmultiply(src: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(src.len());
    for px in src.chunks_exact(4) {
        let a = px[3] as u32;
        if a == 0 {
            out.extend_from_slice(&[0, 0, 0, 0]);
        } else {
            out.push(((px[0] as u32 * 255) / a).min(255) as u8);
            out.push(((px[1] as u32 * 255) / a).min(255) as u8);
            out.push(((px[2] as u32 * 255) / a).min(255) as u8);
            out.push(a as u8);
        }
    }
    out
}

pub fn svg_image(svg: &str, px: u32) -> Option<ColorImage> {
    let opt = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_str(svg, &opt).ok()?;
    let mut pm = resvg::tiny_skia::Pixmap::new(px, px)?;
    let size = tree.size();
    let s = px as f32 / size.width().max(size.height());
    let tx = (px as f32 - size.width() * s) * 0.5;
    let ty = (px as f32 - size.height() * s) * 0.5;
    let ts = resvg::tiny_skia::Transform::from_translate(tx, ty).pre_scale(s, s);
    resvg::render(&tree, ts, &mut pm.as_mut());
    let bytes = unmultiply(pm.data());
    Some(ColorImage::from_rgba_unmultiplied([px as usize, px as usize], &bytes))
}

pub struct Assets {
    tex: HashMap<String, TextureHandle>,
}

impl Assets {
    pub fn new() -> Self {
        Self { tex: HashMap::new() }
    }

    pub fn piece(&mut self, ctx: &Context, set: usize, white: bool, role: shakmaty::Role) -> TextureHandle {
        let name = PIECE_SETS[set.min(PIECE_SETS.len().saturating_sub(1))];
        let letter = match role {
            shakmaty::Role::Pawn => "P",
            shakmaty::Role::Knight => "N",
            shakmaty::Role::Bishop => "B",
            shakmaty::Role::Rook => "R",
            shakmaty::Role::Queen => "Q",
            shakmaty::Role::King => "K",
        };
        let file = format!("{}{}.svg", if white { "w" } else { "b" }, letter);
        let key = format!("{name}/{file}");
        if let Some(t) = self.tex.get(&key) {
            return t.clone();
        }
        let path = root().join("lichess").join("pieces").join(name).join(&file);
        let svg = std::fs::read_to_string(&path).unwrap_or_default();
        let img = svg_image(&svg, 256).unwrap_or_else(|| {
            ColorImage::from_rgba_unmultiplied([2, 2], &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
        });
        let t = ctx.load_texture(key.clone(), img, TextureOptions::LINEAR);
        self.tex.insert(key, t.clone());
        t
    }

    pub fn badge(&mut self, ctx: &Context, cls: &str) -> Option<TextureHandle> {
        let key = format!("badge/{cls}");
        if let Some(t) = self.tex.get(&key) {
            return Some(t.clone());
        }
        let path = root().join("assets").join("badges").join(format!("{cls}.png"));
        let bytes = std::fs::read(path).ok()?;
        let img = image::load_from_memory(&bytes).ok()?.to_rgba8();
        let (w, h) = (img.width(), img.height());
        let data = img.into_raw();
        let ci = ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &data);
        let t = ctx.load_texture(key.clone(), ci, TextureOptions::LINEAR);
        self.tex.insert(key, t.clone());
        Some(t)
    }
}
