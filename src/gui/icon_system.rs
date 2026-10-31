//! EdenExplorer's icon boundary.
//!
//! UI code should use [`AppIcon`] and the helpers in this module rather than
//! depending on a particular icon set, font, or egui integration crate. The
//! default backend remains Phosphor while the SVG backend is migrated in.

use eframe::egui;
use egui::{Align2, Color32, FontId, Image, Rect, Response, Sense, Ui, Vec2};
use std::fmt;
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IconBackend {
    Phosphor = 0,
    Lucide = 1,
}

// Lucide is the production backend. Phosphor remains available for an
// immediate compatibility rollback while the SVG migration settles.
static ACTIVE_BACKEND: AtomicU8 = AtomicU8::new(IconBackend::Lucide as u8);

pub fn icon_backend() -> IconBackend {
    match ACTIVE_BACKEND.load(Ordering::Relaxed) {
        1 => IconBackend::Lucide,
        _ => IconBackend::Phosphor,
    }
}

pub fn set_icon_backend(backend: IconBackend) {
    ACTIVE_BACKEND.store(backend as u8, Ordering::Relaxed);
}

pub fn uses_phosphor_backend() -> bool {
    icon_backend() == IconBackend::Phosphor
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppIcon {
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowCounterClockwise,
    ArrowsClockwise,
    ArrowsInLineHorizontal,
    CaretDown,
    CaretLeft,
    CaretRight,
    CaretUp,
    Check,
    Copy,
    DotsSixVertical,
    File,
    FilePlus,
    FileText,
    FolderOpen,
    FolderPlus,
    FolderSimple,
    Image,
    ImageSquare,
    ImagesSquare,
    Info,
    List,
    Minus,
    Moon,
    MusicNotes,
    Palette,
    PencilSimple,
    Plus,
    PushPin,
    Question,
    SidebarSimple,
    Sliders,
    SplitHorizontal,
    Square,
    Star,
    Sun,
    Tabs,
    Tag,
    Terminal,
    Trash,
    VideoCamera,
    X,
    DownloadSimple,
}

impl AppIcon {
    #[cfg(test)]
    pub const ALL: &'static [Self] = &[
        Self::ArrowLeft,
        Self::ArrowRight,
        Self::ArrowUp,
        Self::ArrowCounterClockwise,
        Self::ArrowsClockwise,
        Self::ArrowsInLineHorizontal,
        Self::CaretDown,
        Self::CaretLeft,
        Self::CaretRight,
        Self::CaretUp,
        Self::Check,
        Self::Copy,
        Self::DotsSixVertical,
        Self::File,
        Self::FilePlus,
        Self::FileText,
        Self::FolderOpen,
        Self::FolderPlus,
        Self::FolderSimple,
        Self::Image,
        Self::ImageSquare,
        Self::ImagesSquare,
        Self::Info,
        Self::List,
        Self::Minus,
        Self::Moon,
        Self::MusicNotes,
        Self::Palette,
        Self::PencilSimple,
        Self::Plus,
        Self::PushPin,
        Self::Question,
        Self::SidebarSimple,
        Self::Sliders,
        Self::SplitHorizontal,
        Self::Square,
        Self::Star,
        Self::Sun,
        Self::Tabs,
        Self::Tag,
        Self::Terminal,
        Self::Trash,
        Self::VideoCamera,
        Self::X,
        Self::DownloadSimple,
    ];
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IconStyle {
    Regular,
    Filled,
}

impl Default for IconStyle {
    fn default() -> Self {
        Self::Regular
    }
}

impl AppIcon {
    pub fn regular(self) -> StyledIcon {
        StyledIcon {
            icon: self,
            style: IconStyle::Regular,
        }
    }

    pub fn filled(self) -> StyledIcon {
        StyledIcon {
            icon: self,
            style: IconStyle::Filled,
        }
    }

    fn phosphor_glyph(self, style: IconStyle) -> &'static str {
        use egui_phosphor::fill;
        use egui_phosphor::regular;

        match style {
            IconStyle::Filled => match self {
                Self::PushPin => fill::PUSH_PIN,
                Self::Star => fill::STAR,
                _ => self.phosphor_glyph(IconStyle::Regular),
            },
            IconStyle::Regular => match self {
                Self::ArrowLeft => regular::ARROW_LEFT,
                Self::ArrowRight => regular::ARROW_RIGHT,
                Self::ArrowUp => regular::ARROW_UP,
                Self::ArrowCounterClockwise => regular::ARROW_COUNTER_CLOCKWISE,
                Self::ArrowsClockwise => regular::ARROWS_CLOCKWISE,
                Self::ArrowsInLineHorizontal => regular::ARROWS_IN_LINE_HORIZONTAL,
                Self::CaretDown => regular::CARET_DOWN,
                Self::CaretLeft => regular::CARET_LEFT,
                Self::CaretRight => regular::CARET_RIGHT,
                Self::CaretUp => regular::CARET_UP,
                Self::Check => regular::CHECK,
                Self::Copy => regular::COPY,
                Self::DotsSixVertical => regular::DOTS_SIX_VERTICAL,
                Self::File => regular::FILE,
                Self::FilePlus => regular::FILE_PLUS,
                Self::FileText => regular::FILE_TEXT,
                Self::FolderOpen => regular::FOLDER_OPEN,
                Self::FolderPlus => regular::FOLDER_PLUS,
                Self::FolderSimple => regular::FOLDER_SIMPLE,
                Self::Image => regular::IMAGE,
                Self::ImageSquare => regular::IMAGE_SQUARE,
                Self::ImagesSquare => regular::IMAGES_SQUARE,
                Self::Info => regular::INFO,
                Self::List => regular::LIST,
                Self::Minus => regular::MINUS,
                Self::Moon => regular::MOON,
                Self::MusicNotes => regular::MUSIC_NOTES,
                Self::Palette => regular::PALETTE,
                Self::PencilSimple => regular::PENCIL_SIMPLE,
                Self::Plus => regular::PLUS,
                Self::PushPin => regular::PUSH_PIN,
                Self::Question => regular::QUESTION,
                Self::SidebarSimple => regular::SIDEBAR_SIMPLE,
                Self::Sliders => regular::SLIDERS,
                Self::SplitHorizontal => regular::SPLIT_HORIZONTAL,
                Self::Square => regular::SQUARE,
                Self::Star => regular::STAR,
                Self::Sun => regular::SUN,
                Self::Tabs => regular::TABS,
                Self::Tag => regular::TAG,
                Self::Terminal => regular::TERMINAL,
                Self::Trash => regular::TRASH,
                Self::VideoCamera => regular::VIDEO_CAMERA,
                Self::X => regular::X,
                Self::DownloadSimple => regular::DOWNLOAD_SIMPLE,
            },
        }
    }

    fn lucide_name(self) -> &'static str {
        match self {
            Self::ArrowLeft => "arrow-left",
            Self::ArrowRight => "arrow-right",
            Self::ArrowUp => "arrow-up",
            Self::ArrowCounterClockwise => "undo-2",
            Self::ArrowsClockwise => "refresh-cw",
            Self::ArrowsInLineHorizontal => "panel-left-close",
            Self::CaretDown => "chevron-down",
            Self::CaretLeft => "chevron-left",
            Self::CaretRight => "chevron-right",
            Self::CaretUp => "chevron-up",
            Self::Check => "check",
            Self::Copy => "copy",
            Self::DotsSixVertical => "grip-vertical",
            Self::File => "file",
            Self::FilePlus => "file-plus",
            Self::FileText => "file-text",
            Self::FolderOpen => "folder-open",
            Self::FolderPlus => "folder-plus",
            Self::FolderSimple => "folder",
            Self::Image => "image",
            Self::ImageSquare => "image",
            Self::ImagesSquare => "images",
            Self::Info => "info",
            Self::List => "list",
            Self::Minus => "minus",
            Self::Moon => "moon",
            Self::MusicNotes => "audio-lines",
            Self::Palette => "palette",
            Self::PencilSimple => "pencil",
            Self::Plus => "plus",
            Self::PushPin => "pin",
            Self::Question => "circle-question-mark",
            Self::SidebarSimple => "panel-left",
            Self::Sliders => "sliders-horizontal",
            Self::SplitHorizontal => "columns-2",
            Self::Square => "square",
            Self::Star => "star",
            Self::Sun => "sun",
            Self::Tabs => "panel-top",
            Self::Tag => "tag",
            Self::Terminal => "square-terminal",
            Self::Trash => "trash",
            Self::VideoCamera => "video",
            Self::X => "x",
            Self::DownloadSimple => "download",
        }
    }

    fn lucide_svg(self) -> &'static str {
        match self {
            Self::ArrowLeft => include_str!("../assets/icons/lucide/arrow-left.svg"),
            Self::ArrowRight => include_str!("../assets/icons/lucide/arrow-right.svg"),
            Self::ArrowUp => include_str!("../assets/icons/lucide/arrow-up.svg"),
            Self::ArrowCounterClockwise => include_str!("../assets/icons/lucide/undo-2.svg"),
            Self::ArrowsClockwise => include_str!("../assets/icons/lucide/refresh-cw.svg"),
            Self::ArrowsInLineHorizontal => {
                include_str!("../assets/icons/lucide/panel-left-close.svg")
            }
            Self::CaretDown => include_str!("../assets/icons/lucide/chevron-down.svg"),
            Self::CaretLeft => include_str!("../assets/icons/lucide/chevron-left.svg"),
            Self::CaretRight => include_str!("../assets/icons/lucide/chevron-right.svg"),
            Self::CaretUp => include_str!("../assets/icons/lucide/chevron-up.svg"),
            Self::Check => include_str!("../assets/icons/lucide/check.svg"),
            Self::Copy => include_str!("../assets/icons/lucide/copy.svg"),
            Self::DotsSixVertical => include_str!("../assets/icons/lucide/grip-vertical.svg"),
            Self::File => include_str!("../assets/icons/lucide/file.svg"),
            Self::FilePlus => include_str!("../assets/icons/lucide/file-plus.svg"),
            Self::FileText => include_str!("../assets/icons/lucide/file-text.svg"),
            Self::FolderOpen => include_str!("../assets/icons/lucide/folder-open.svg"),
            Self::FolderPlus => include_str!("../assets/icons/lucide/folder-plus.svg"),
            Self::FolderSimple => include_str!("../assets/icons/lucide/folder.svg"),
            Self::Image | Self::ImageSquare => include_str!("../assets/icons/lucide/image.svg"),
            Self::ImagesSquare => include_str!("../assets/icons/lucide/images.svg"),
            Self::Info => include_str!("../assets/icons/lucide/info.svg"),
            Self::List => include_str!("../assets/icons/lucide/list.svg"),
            Self::Minus => include_str!("../assets/icons/lucide/minus.svg"),
            Self::Moon => include_str!("../assets/icons/lucide/moon.svg"),
            Self::MusicNotes => include_str!("../assets/icons/lucide/audio-lines.svg"),
            Self::Palette => include_str!("../assets/icons/lucide/palette.svg"),
            Self::PencilSimple => include_str!("../assets/icons/lucide/pencil.svg"),
            Self::Plus => include_str!("../assets/icons/lucide/plus.svg"),
            Self::PushPin => include_str!("../assets/icons/lucide/pin.svg"),
            Self::Question => include_str!("../assets/icons/lucide/circle-question-mark.svg"),
            Self::SidebarSimple => include_str!("../assets/icons/lucide/panel-left.svg"),
            Self::Sliders => include_str!("../assets/icons/lucide/sliders-horizontal.svg"),
            Self::SplitHorizontal => include_str!("../assets/icons/lucide/columns-2.svg"),
            Self::Square => include_str!("../assets/icons/lucide/square.svg"),
            Self::Star => include_str!("../assets/icons/lucide/star.svg"),
            Self::Sun => include_str!("../assets/icons/lucide/sun.svg"),
            Self::Tabs => include_str!("../assets/icons/lucide/panel-top.svg"),
            Self::Tag => include_str!("../assets/icons/lucide/tag.svg"),
            Self::Terminal => include_str!("../assets/icons/lucide/square-terminal.svg"),
            Self::Trash => include_str!("../assets/icons/lucide/trash.svg"),
            Self::VideoCamera => include_str!("../assets/icons/lucide/video.svg"),
            Self::X => include_str!("../assets/icons/lucide/x.svg"),
            Self::DownloadSimple => include_str!("../assets/icons/lucide/download.svg"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StyledIcon {
    pub icon: AppIcon,
    pub style: IconStyle,
}

impl StyledIcon {
    /// Compatibility access for painter-only overlays that cannot host an
    /// egui image widget. Normal UI code should use `paint_icon` instead.
    pub fn phosphor_glyph(self) -> &'static str {
        self.icon.phosphor_glyph(self.style)
    }
}

impl fmt::Display for StyledIcon {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.icon.phosphor_glyph(self.style))
    }
}

fn icon_rect(ui: &mut Ui, size: f32) -> (Rect, Response) {
    ui.allocate_exact_size(Vec2::splat(size), Sense::click())
}

pub fn draw_icon(ui: &mut Ui, icon: StyledIcon, size: f32, color: Color32) -> Response {
    let (rect, response) = icon_rect(ui, size);
    paint_icon(ui, rect, icon, size, color);
    response
}

pub fn paint_icon(ui: &mut Ui, rect: Rect, icon: StyledIcon, size: f32, color: Color32) {
    match icon_backend() {
        IconBackend::Phosphor => {
            let font = FontId::proportional(size);
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                icon.icon.phosphor_glyph(icon.style),
                font,
                color,
            );
        }
        IconBackend::Lucide => {
            // Lucide SVGs use `currentColor`. An SVG loaded as an image has
            // no inherited CSS color, so replace it before handing the bytes
            // to the loader. Tinting black SVG pixels would remain black.
            let color_key = format!(
                "{:02x}{:02x}{:02x}{:02x}",
                color.r(),
                color.g(),
                color.b(),
                color.a()
            );
            let color_svg = icon
                .icon
                .lucide_svg()
                .replace("currentColor", &format!("#{color_key}"));
            let uri = format!(
                "bytes://eden-icon/{}-{}.svg",
                icon.icon.lucide_name(),
                color_key
            );
            let image =
                Image::from_bytes(uri, color_svg.into_bytes()).fit_to_exact_size(Vec2::splat(size));
            ui.put(rect, image);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AppIcon;

    #[test]
    fn every_app_icon_has_a_lucide_asset() {
        for icon in AppIcon::ALL {
            let svg = icon.lucide_svg();
            assert!(!svg.is_empty(), "missing Lucide asset for {icon:?}");
            assert!(
                svg.trim_start().starts_with("<svg"),
                "invalid Lucide SVG asset for {icon:?}"
            );
        }
    }
}
