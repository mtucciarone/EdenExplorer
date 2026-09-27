//! Layouts for the Disk Usage charts (drawn by
//! `gui::windows::disk_usage_charts`):
//!
//! - **Treemap**: every file is a rectangle whose area is its size, nested
//!   inside its folders, laid out with the "squarified" algorithm (Bruls,
//!   Huizing & van Wijk) so blocks stay close to square. It's shaded with
//!   WinDirStat-style "cushions" (van Wijk & van de Wetering): each nesting
//!   level adds a gentle bump, so the folder structure shows as ridges even
//!   without borders.
//! - **Sunburst**: the folder in the middle, its contents as ring segments
//!   around it, their contents on the next ring out, and so on.
//!
//! Both are plain geometry over `DirNode` with no UI or disk access, so
//! they're tested directly; drawing uses the results.

use crate::core::disk_usage::DirNode;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn area(&self) -> f32 {
        self.w * self.h
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

/// Splits `rect` into one rectangle per size (sizes largest first, all
/// > 0), each with area proportional to its size.
pub fn squarify(sizes: &[u64], rect: Rect) -> Vec<Rect> {
    let total: f64 = sizes.iter().map(|&s| s as f64).sum();
    let mut out = Vec::with_capacity(sizes.len());
    if sizes.is_empty() || total <= 0.0 || rect.w <= 0.0 || rect.h <= 0.0 {
        return out;
    }
    let scale = (rect.w as f64 * rect.h as f64) / total;
    let areas: Vec<f64> = sizes.iter().map(|&s| s as f64 * scale).collect();

    let (mut x, mut y, mut w, mut h) = (rect.x as f64, rect.y as f64, rect.w as f64, rect.h as f64);
    let mut start = 0;
    while start < areas.len() {
        let side = w.min(h);
        // Grow the row while it makes the worst aspect ratio better.
        let mut end = start + 1;
        let mut row_sum = areas[start];
        let mut best = worst(&areas[start..end], row_sum, side);
        while end < areas.len() {
            let next_sum = row_sum + areas[end];
            let next = worst(&areas[start..=end], next_sum, side);
            if next > best {
                break;
            }
            best = next;
            row_sum = next_sum;
            end += 1;
        }
        // Lay the row along the shorter side.
        let thickness = if side > 0.0 { row_sum / side } else { 0.0 };
        let mut offset = 0.0;
        for &area in &areas[start..end] {
            let length = if thickness > 0.0 { area / thickness } else { 0.0 };
            let r = if w >= h {
                Rect { x: x as f32, y: (y + offset) as f32, w: thickness as f32, h: length as f32 }
            } else {
                Rect { x: (x + offset) as f32, y: y as f32, w: length as f32, h: thickness as f32 }
            };
            out.push(r);
            offset += length;
        }
        if w >= h {
            x += thickness;
            w -= thickness;
        } else {
            y += thickness;
            h -= thickness;
        }
        start = end;
    }
    out
}

/// The worst (largest) aspect ratio in a row of `areas` laid along `side`.
fn worst(areas: &[f64], sum: f64, side: f64) -> f64 {
    if sum <= 0.0 || side <= 0.0 {
        return f64::MAX;
    }
    let side2 = side * side;
    let sum2 = sum * sum;
    let max = areas.iter().cloned().fold(0.0, f64::max);
    let min = areas.iter().cloned().fold(f64::MAX, f64::min);
    (side2 * max / sum2).max(sum2 / (side2 * min))
}

/// What a treemap block stands for.
#[derive(Clone, Debug, PartialEq)]
pub enum BlockKind {
    /// A file; `ext` is its extension (lowercase, no dot).
    File { ext: String },
    /// A folder too small on screen to split into its contents.
    Folder,
}

/// One drawn block.
#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub rect: Rect,
    /// Folder names from the chart's root down to this item, then (for a
    /// file) the file name.
    pub path: Vec<String>,
    pub kind: BlockKind,
    pub size: u64,
    /// Cushion surface: height = s[0]x² + s[1]x + s[2]y² + s[3]y.
    pub surface: [f32; 4],
}

/// A folder's outline (for highlighting the selected folder).
#[derive(Clone, Debug, PartialEq)]
pub struct FolderOutline {
    pub rect: Rect,
    pub path: Vec<String>,
    pub size: u64,
}

#[derive(Default)]
pub struct Treemap {
    pub blocks: Vec<Block>,
    pub folders: Vec<FolderOutline>,
}

impl Treemap {
    /// The block under (x, y).
    pub fn block_at(&self, x: f32, y: f32) -> Option<&Block> {
        self.blocks.iter().find(|b| b.rect.contains(x, y))
    }

    /// The outline of the folder at `path`.
    pub fn folder(&self, path: &[String]) -> Option<&FolderOutline> {
        self.folders.iter().find(|f| f.path == path)
    }
}

/// Cushion height of the outermost level; each level down is `FALLOFF`
/// times lower.
const CUSHION_HEIGHT: f32 = 0.5;
const CUSHION_FALLOFF: f32 = 0.75;
/// Items smaller than this many square pixels aren't split further.
const MIN_AREA: f32 = 2.0;

fn add_ridge(surface: &mut [f32; 4], rect: Rect, height: f32) {
    if rect.w > 0.0 {
        let (x1, x2) = (rect.x, rect.x + rect.w);
        surface[0] -= 4.0 * height / (x2 - x1);
        surface[1] += 4.0 * height * (x1 + x2) / (x2 - x1);
    }
    if rect.h > 0.0 {
        let (y1, y2) = (rect.y, rect.y + rect.h);
        surface[2] -= 4.0 * height / (y2 - y1);
        surface[3] += 4.0 * height * (y1 + y2) / (y2 - y1);
    }
}

/// Lays out `root` (the folder being shown) in `bounds`.
pub fn layout(root: &DirNode, bounds: Rect) -> Treemap {
    let mut map = Treemap::default();
    let mut path = Vec::new();
    layout_dir(root, bounds, 0, [0.0; 4], &mut path, &mut map);
    map
}

fn layout_dir(dir: &DirNode, rect: Rect, depth: u32, surface: [f32; 4], path: &mut Vec<String>, map: &mut Treemap) {
    if dir.size == 0 || rect.area() <= 0.0 {
        return;
    }
    let mut surface = surface;
    add_ridge(&mut surface, rect, CUSHION_HEIGHT * CUSHION_FALLOFF.powi(depth as i32));
    map.folders.push(FolderOutline {
        rect,
        path: path.clone(),
        size: dir.size,
    });
    if rect.area() < MIN_AREA * 4.0 {
        map.blocks.push(Block {
            rect,
            path: path.clone(),
            kind: BlockKind::Folder,
            size: dir.size,
            surface,
        });
        return;
    }

    // Folders and files together, largest first.
    enum Item<'a> {
        Dir(&'a DirNode),
        File(&'a crate::core::disk_usage::FileEntry),
    }
    let mut items: Vec<(u64, Item)> = dir
        .dirs
        .iter()
        .filter(|d| d.size > 0)
        .map(|d| (d.size, Item::Dir(d)))
        .chain(dir.files.iter().filter(|f| f.size > 0).map(|f| (f.size, Item::File(f))))
        .collect();
    items.sort_by(|a, b| b.0.cmp(&a.0));
    let sizes: Vec<u64> = items.iter().map(|(s, _)| *s).collect();
    let rects = squarify(&sizes, rect);

    // Everything too small to see is merged into one block at the end.
    let mut rest: Option<(Rect, u64)> = None;
    for ((size, item), r) in items.iter().zip(rects) {
        if r.area() < MIN_AREA {
            rest = Some(match rest {
                None => (r, *size),
                Some((acc, total)) => (union(acc, r), total + size),
            });
            continue;
        }
        match item {
            Item::Dir(child) => {
                path.push(child.name.to_string());
                layout_dir(child, r, depth + 1, surface, path, map);
                path.pop();
            }
            Item::File(file) => {
                let mut s = surface;
                add_ridge(&mut s, r, CUSHION_HEIGHT * CUSHION_FALLOFF.powi(depth as i32 + 1));
                let mut file_path = path.clone();
                file_path.push(file.name.to_string());
                map.blocks.push(Block {
                    rect: r,
                    path: file_path,
                    kind: BlockKind::File {
                        ext: crate::core::disk_usage_stats::extension_of(&file.name),
                    },
                    size: file.size,
                    surface: s,
                });
            }
        }
    }
    if let Some((r, size)) = rest {
        map.blocks.push(Block {
            rect: r,
            path: path.clone(),
            kind: BlockKind::Folder,
            size,
            surface,
        });
    }
}

fn union(a: Rect, b: Rect) -> Rect {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    let r = (a.x + a.w).max(b.x + b.w);
    let bottom = (a.y + a.h).max(b.y + b.h);
    Rect { x, y, w: r - x, h: bottom - y }
}

/// Renders the cushion-shaded treemap into RGBA pixels (`width` x
/// `height`, the same size the layout was made for). `color_of` gives each
/// block's base color and whether it's dimmed (not matching a highlighted
/// type).
pub fn render(map: &Treemap, width: usize, height: usize, mut color_of: impl FnMut(&Block) -> ([u8; 3], bool)) -> Vec<u8> {
    let mut pixels = vec![0u8; width * height * 4];
    // Light from the top left, like WinDirStat.
    let light = {
        let (lx, ly, lz) = (-1.0f32, -1.0f32, 10.0f32);
        let len = (lx * lx + ly * ly + lz * lz).sqrt();
        (lx / len, ly / len, lz / len)
    };
    const AMBIENT: f32 = 0.25;
    const DIFFUSE: f32 = 0.9;
    for block in &map.blocks {
        let (rgb, dimmed) = color_of(block);
        let s = block.surface;
        let x0 = block.rect.x.round().max(0.0) as usize;
        let y0 = block.rect.y.round().max(0.0) as usize;
        let x1 = ((block.rect.x + block.rect.w).round() as usize).min(width);
        let y1 = ((block.rect.y + block.rect.h).round() as usize).min(height);
        for py in y0..y1 {
            let fy = py as f32 + 0.5;
            let ny = -(2.0 * s[2] * fy + s[3]);
            for px in x0..x1 {
                let fx = px as f32 + 0.5;
                let nx = -(2.0 * s[0] * fx + s[1]);
                let cos = (nx * light.0 + ny * light.1 + light.2) / (nx * nx + ny * ny + 1.0).sqrt();
                let mut intensity = AMBIENT + DIFFUSE * cos.max(0.0);
                if dimmed {
                    intensity *= 0.3;
                }
                let i = (py * width + px) * 4;
                pixels[i] = (rgb[0] as f32 * intensity).min(255.0) as u8;
                pixels[i + 1] = (rgb[1] as f32 * intensity).min(255.0) as u8;
                pixels[i + 2] = (rgb[2] as f32 * intensity).min(255.0) as u8;
                pixels[i + 3] = 255;
            }
        }
    }
    pixels
}

/// One ring segment of the sunburst. Angles are in radians from 12
/// o'clock, clockwise; radii are ring numbers (1 = innermost ring).
#[derive(Clone, Debug, PartialEq)]
pub struct Segment {
    pub start: f32,
    pub end: f32,
    pub ring: u32,
    pub path: Vec<String>,
    pub is_dir: bool,
    pub ext: String,
    pub size: u64,
    /// Index (in size order) of the ring-1 item this segment is under, for
    /// giving each branch its own hue.
    pub branch: usize,
}

/// Segments narrower than this (radians) aren't drawn.
const MIN_ANGLE: f32 = 0.004;

/// Lays out `root` as rings, up to `max_rings` deep.
pub fn sunburst(root: &DirNode, max_rings: u32) -> Vec<Segment> {
    let mut out = Vec::new();
    if root.size == 0 {
        return out;
    }
    let mut path = Vec::new();
    sunburst_dir(root, 0.0, std::f32::consts::TAU, 1, max_rings, None, &mut path, &mut out);
    out
}

#[allow(clippy::too_many_arguments)]
fn sunburst_dir(
    dir: &DirNode,
    start: f32,
    end: f32,
    ring: u32,
    max_rings: u32,
    branch: Option<usize>,
    path: &mut Vec<String>,
    out: &mut Vec<Segment>,
) {
    if ring > max_rings || dir.size == 0 {
        return;
    }
    let span = end - start;
    let mut angle = start;
    let mut index = 0usize;
    // Folders first, then files - both largest first.
    for child in dir.dirs.iter().filter(|d| d.size > 0) {
        let a = span * (child.size as f64 / dir.size as f64) as f32;
        let b = branch.unwrap_or(index);
        if a >= MIN_ANGLE {
            path.push(child.name.to_string());
            out.push(Segment {
                start: angle,
                end: angle + a,
                ring,
                path: path.clone(),
                is_dir: true,
                ext: String::new(),
                size: child.size,
                branch: b,
            });
            sunburst_dir(child, angle, angle + a, ring + 1, max_rings, Some(b), path, out);
            path.pop();
        }
        angle += a;
        index += 1;
    }
    for file in dir.files.iter().filter(|f| f.size > 0) {
        let a = span * (file.size as f64 / dir.size as f64) as f32;
        if a >= MIN_ANGLE {
            let mut file_path = path.clone();
            file_path.push(file.name.to_string());
            out.push(Segment {
                start: angle,
                end: angle + a,
                ring,
                path: file_path,
                is_dir: false,
                ext: crate::core::disk_usage_stats::extension_of(&file.name),
                size: file.size,
                branch: branch.unwrap_or(index),
            });
        }
        angle += a;
        index += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::disk_usage::FileEntry;

    fn file(name: &str, size: u64) -> FileEntry {
        FileEntry {
            name: name.into(),
            size,
            allocated: size,
            modified: 0,
        }
    }

    fn dir(name: &str, dirs: Vec<DirNode>, files: Vec<FileEntry>) -> DirNode {
        let mut node = DirNode {
            name: name.into(),
            dirs,
            files,
            ..Default::default()
        };
        node.recompute_totals();
        node.sort_children();
        node
    }

    fn sample() -> DirNode {
        dir(
            "root",
            vec![
                dir("a", vec![], vec![file("big.mp4", 600), file("b.txt", 100)]),
                dir("c", vec![dir("d", vec![], vec![file("x.jpg", 150)])], vec![]),
            ],
            vec![file("top.zip", 150)],
        )
    }

    #[test]
    fn squarify_areas_are_proportional_and_tile_the_rectangle() {
        let bounds = Rect { x: 10.0, y: 20.0, w: 600.0, h: 400.0 };
        let sizes = [500, 300, 200, 100, 50, 25, 25];
        let rects = squarify(&sizes, bounds);
        assert_eq!(rects.len(), sizes.len());
        let total: u64 = sizes.iter().sum();
        let area: f32 = rects.iter().map(|r| r.area()).sum();
        assert!((area - bounds.area()).abs() < 1.0);
        for (r, s) in rects.iter().zip(sizes) {
            let expected = bounds.area() * s as f32 / total as f32;
            assert!((r.area() - expected).abs() < 1.0, "{r:?} vs {expected}");
            assert!(r.x >= bounds.x - 0.01 && r.x + r.w <= bounds.x + bounds.w + 0.01);
            assert!(r.y >= bounds.y - 0.01 && r.y + r.h <= bounds.y + bounds.h + 0.01);
            // Squarified: no sliver thinner than 1:10 for these sizes.
            assert!(r.w.max(r.h) / r.w.min(r.h) < 10.0, "{r:?}");
        }
        assert!(squarify(&[], bounds).is_empty());
    }

    #[test]
    fn layout_places_every_file_inside_its_folder() {
        let bounds = Rect { x: 0.0, y: 0.0, w: 400.0, h: 250.0 };
        let map = layout(&sample(), bounds);
        let names: Vec<String> = map.blocks.iter().map(|b| b.path.join("/")).collect();
        for expected in ["a/big.mp4", "a/b.txt", "c/d/x.jpg", "top.zip"] {
            assert!(names.contains(&expected.to_string()), "{expected} in {names:?}");
        }
        let block = |p: &str| map.blocks.iter().find(|b| b.path.join("/") == p).unwrap();
        let folder_a = map.folder(&["a".to_string()]).unwrap();
        let big = block("a/big.mp4");
        assert!(big.rect.x >= folder_a.rect.x - 0.01 && big.rect.x + big.rect.w <= folder_a.rect.x + folder_a.rect.w + 0.01);
        assert_eq!(big.kind, BlockKind::File { ext: "mp4".into() });
        assert!((big.rect.area() / bounds.area() - 0.6).abs() < 0.01);
        let hit = map.block_at(big.rect.x + 1.0, big.rect.y + 1.0).unwrap();
        assert_eq!(hit.path, big.path);
    }

    #[test]
    fn tiny_items_are_merged_instead_of_dropped() {
        let files: Vec<FileEntry> = (0..500).map(|i| file(&format!("f{i}"), 1)).collect();
        let root = dir("r", vec![], [vec![file("huge", 1_000_000)], files].concat());
        let bounds = Rect { x: 0.0, y: 0.0, w: 100.0, h: 100.0 };
        let map = layout(&root, bounds);
        let total: u64 = map.blocks.iter().map(|b| b.size).sum();
        assert_eq!(total, root.size, "the rest is still accounted for");
        assert!(map.blocks.len() < 10);
    }

    #[test]
    fn cushions_are_lit_from_the_top_left() {
        let bounds = Rect { x: 0.0, y: 0.0, w: 40.0, h: 40.0 };
        let root = dir("r", vec![], vec![file("only", 10)]);
        let map = layout(&root, bounds);
        let pixels = render(&map, 40, 40, |_| ([200, 200, 200], false));
        let at = |x: usize, y: usize| pixels[(y * 40 + x) * 4] as i32;
        assert!(at(10, 10) > at(30, 30), "brighter towards the light");
        assert!(at(20, 20) > 100);
        assert_eq!(pixels[3], 255);
        let dim = render(&map, 40, 40, |_| ([200, 200, 200], true));
        assert!((dim[(20 * 40 + 20) * 4] as i32) < at(20, 20) / 2);
    }

    #[test]
    fn sunburst_rings_follow_the_folder_structure() {
        let segments = sunburst(&sample(), 3);
        let find = |p: &str| segments.iter().find(|s| s.path.join("/") == p).unwrap();
        let a = find("a");
        assert_eq!(a.ring, 1);
        assert!((a.end - a.start - std::f32::consts::TAU * 700.0 / 1000.0).abs() < 1e-3);
        let big = find("a/big.mp4");
        assert_eq!(big.ring, 2);
        assert!(big.start >= a.start && big.end <= a.end + 1e-4);
        assert_eq!(big.branch, a.branch);
        assert_eq!(find("c/d/x.jpg").ring, 3);
        assert!(sunburst(&sample(), 2).iter().all(|s| s.ring <= 2));
        let c = find("c");
        assert_ne!(c.branch, a.branch);
    }
}
