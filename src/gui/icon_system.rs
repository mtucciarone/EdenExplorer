//! EdenExplorer's icon boundary.
//!
//! UI code should use [`AppIcon`] and the helpers in this module rather than
//! depending on a particular icon set, font, or egui integration crate. The
//! Icons are rendered from vendored Lucide SVG assets and do not use egui font
//! glyphs.

use egui::{Color32, Image, Rect, Response, Sense, Ui, Vec2};

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

fn icon_rect(ui: &mut Ui, size: f32) -> (Rect, Response) {
    ui.allocate_exact_size(Vec2::splat(size), Sense::click())
}

pub fn draw_icon(ui: &mut Ui, icon: StyledIcon, size: f32, color: Color32) -> Response {
    let (rect, response) = icon_rect(ui, size);
    paint_icon(ui, rect, icon, size, color);
    response
}

pub fn paint_icon(ui: &mut Ui, rect: Rect, icon: StyledIcon, size: f32, color: Color32) {
    // Lucide SVGs use `currentColor`. An SVG loaded as an image has no
    // inherited CSS color, so replace it before handing the bytes to the
    // loader. Tinting black SVG pixels would remain black.
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
    let image = Image::from_bytes(uri, color_svg.into_bytes()).fit_to_exact_size(Vec2::splat(size));
    // This renderer is also used after callers have already allocated their
    // layout rect (for example sidebar rows). Paint directly into the
    // requested icon-sized rect so the icon does not advance egui's layout
    // cursor a second time or stretch across a larger button hitbox.
    let icon_rect = Rect::from_center_size(rect.center(), Vec2::splat(size));
    image.paint_at(ui, icon_rect);
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
