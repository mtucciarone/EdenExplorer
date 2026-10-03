//! Draws Material Icon Theme icons (`core::material_icons`): each SVG is
//! rendered once at the size and screen scale it's shown at and kept as a
//! texture.

use crate::core::material_icons::MaterialIcon;
use eframe::egui;
use std::collections::HashMap;
use std::sync::Mutex;

type Textures = HashMap<(MaterialIcon, u32), Option<egui::TextureHandle>>;
static TEXTURES: Mutex<Option<Textures>> = Mutex::new(None);

fn render(icon: MaterialIcon, px: u32) -> Option<egui::ColorImage> {
    let tree = resvg::usvg::Tree::from_data(icon.svg(), &resvg::usvg::Options::default()).ok()?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(px, px)?;
    let size = tree.size();
    let scale = px as f32 / size.width().max(size.height());
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    Some(egui::ColorImage::from_rgba_premultiplied([px as usize, px as usize], pixmap.data()))
}

/// `icon` as an image `size` points across (the light-theme variant when
/// the UI is light).
pub fn image(ui: &egui::Ui, icon: MaterialIcon, size: f32) -> Option<egui::Image<'static>> {
    let icon = icon.themed(!ui.visuals().dark_mode);
    let px = (size * ui.ctx().pixels_per_point()).round().max(1.0) as u32;
    let mut guard = TEXTURES.lock().ok()?;
    let textures = guard.get_or_insert_with(HashMap::new);
    let texture = textures.entry((icon, px)).or_insert_with(|| {
        render(icon, px).map(|image| {
            ui.ctx().load_texture(format!("material-icon-{}-{px}", icon.name()), image, egui::TextureOptions::LINEAR)
        })
    });
    texture.as_ref().map(|t| egui::Image::new((t.id(), egui::vec2(size, size))))
}
