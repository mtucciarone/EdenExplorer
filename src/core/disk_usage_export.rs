//! Exporting a Disk Usage scan: CSV (every folder, every file, or the file
//! types), JSON (the whole tree), a self-contained HTML report, and a
//! plain-text summary for the clipboard. The writers take any `Write`, so
//! they're tested against memory buffers.

use crate::core::disk_usage::{DirNode, largest_files, largest_folders};
use crate::core::disk_usage_stats::{
    AgeBucket, TypeStat, ViewFilter, age_breakdown, category_totals, filetime_now, type_breakdown,
};
use crate::core::utils::files::format_size;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportKind {
    FoldersCsv,
    FilesCsv,
    TypesCsv,
    Json,
    Html,
}

impl ExportKind {
    pub const ALL: [ExportKind; 5] = [
        ExportKind::FoldersCsv,
        ExportKind::FilesCsv,
        ExportKind::TypesCsv,
        ExportKind::Html,
        ExportKind::Json,
    ];

    pub fn i18n_key(self) -> &'static str {
        match self {
            ExportKind::FoldersCsv => "disk_usage_export_folders_csv",
            ExportKind::FilesCsv => "disk_usage_export_files_csv",
            ExportKind::TypesCsv => "disk_usage_export_types_csv",
            ExportKind::Json => "disk_usage_export_json",
            ExportKind::Html => "disk_usage_export_html",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            ExportKind::FoldersCsv | ExportKind::FilesCsv | ExportKind::TypesCsv => "csv",
            ExportKind::Json => "json",
            ExportKind::Html => "html",
        }
    }

    pub fn file_stem(self) -> &'static str {
        match self {
            ExportKind::FoldersCsv => "disk-usage-folders",
            ExportKind::FilesCsv => "disk-usage-files",
            ExportKind::TypesCsv => "disk-usage-types",
            ExportKind::Json => "disk-usage",
            ExportKind::Html => "disk-usage-report",
        }
    }
}

/// A CSV field, quoted when it needs to be.
fn csv(field: &str) -> String {
    if field.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_string()
    }
}

/// A FILETIME as "YYYY-MM-DD HH:MM" (UTC), or empty when unknown.
pub fn filetime_text(ft: i64) -> String {
    const UNIX_EPOCH_AS_FILETIME: i64 = 116_444_736_000_000_000;
    if ft <= UNIX_EPOCH_AS_FILETIME {
        return String::new();
    }
    let secs = (ft - UNIX_EPOCH_AS_FILETIME) / 10_000_000;
    chrono::DateTime::from_timestamp(secs, 0)
        .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default()
}

/// Every folder: path, size, size on disk, files, folders (all counts
/// include subfolders), largest first within each folder.
pub fn write_folders_csv(w: &mut impl Write, tree: &DirNode, root: &Path) -> io::Result<()> {
    writeln!(w, "Path,Size (bytes),On Disk (bytes),Files,Folders")?;
    let mut stack: Vec<(&DirNode, PathBuf)> = vec![(tree, root.to_path_buf())];
    while let Some((dir, path)) = stack.pop() {
        writeln!(
            w,
            "{},{},{},{},{}",
            csv(&path.display().to_string()),
            dir.size,
            dir.allocated,
            dir.file_count,
            dir.dir_count
        )?;
        for child in dir.dirs.iter().rev() {
            stack.push((child, path.join(&*child.name)));
        }
    }
    Ok(())
}

/// Every file: path, size, size on disk, last modified.
pub fn write_files_csv(w: &mut impl Write, tree: &DirNode, root: &Path) -> io::Result<()> {
    writeln!(w, "Path,Size (bytes),On Disk (bytes),Modified (UTC)")?;
    let mut stack: Vec<(&DirNode, PathBuf)> = vec![(tree, root.to_path_buf())];
    while let Some((dir, path)) = stack.pop() {
        for file in &dir.files {
            writeln!(
                w,
                "{},{},{},{}",
                csv(&path.join(&*file.name).display().to_string()),
                file.size,
                file.allocated,
                filetime_text(file.modified)
            )?;
        }
        for child in dir.dirs.iter().rev() {
            stack.push((child, path.join(&*child.name)));
        }
    }
    Ok(())
}

pub fn write_types_csv(w: &mut impl Write, types: &[TypeStat]) -> io::Result<()> {
    writeln!(w, "Extension,Category,Size (bytes),On Disk (bytes),Files,Largest File")?;
    for t in types {
        writeln!(
            w,
            "{},{:?},{},{},{},{}",
            csv(&t.extension),
            t.category,
            t.size,
            t.allocated,
            t.count,
            csv(&t.largest.as_ref().map(|(p, _)| p.display().to_string()).unwrap_or_default())
        )?;
    }
    Ok(())
}

/// The whole tree as JSON: `{"root": ..., "exported": ..., "tree": {...}}`.
pub fn write_json(w: &mut impl Write, tree: &DirNode, root: &Path) -> io::Result<()> {
    #[derive(serde::Serialize)]
    struct Export<'a> {
        root: String,
        exported: String,
        tree: &'a DirNode,
    }
    let export = Export {
        root: root.display().to_string(),
        exported: chrono::Local::now().to_rfc3339(),
        tree,
    };
    serde_json::to_writer(w, &export).map_err(io::Error::other)
}

/// A short plain-text summary: totals, top folders, top types, top files.
pub fn summary_text(tree: &DirNode, root: &Path) -> String {
    let mut out = format!(
        "Disk Usage: {}\n{} ({} on disk) in {} files and {} folders\n",
        root.display(),
        format_size(tree.size),
        format_size(tree.allocated),
        tree.file_count,
        tree.dir_count
    );
    out.push_str("\nLargest folders:\n");
    for dir in tree.dirs.iter().take(10) {
        out.push_str(&format!("  {:>10}  {}\n", format_size(dir.size), dir.name));
    }
    out.push_str("\nFile types:\n");
    for t in type_breakdown(tree, root, &ViewFilter::default()).iter().take(10) {
        let ext = if t.extension.is_empty() { "(none)".to_string() } else { format!(".{}", t.extension) };
        out.push_str(&format!("  {:>10}  {ext} ({} files)\n", format_size(t.size), t.count));
    }
    out.push_str("\nLargest files:\n");
    for f in largest_files(tree, root, 10, &ViewFilter::default()) {
        out.push_str(&format!("  {:>10}  {}\n", format_size(f.size), f.path.display()));
    }
    out
}

fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn pct(part: u64, whole: u64) -> f64 {
    if whole == 0 { 0.0 } else { part as f64 / whole as f64 * 100.0 }
}

/// A self-contained HTML report (inline styles, no scripts).
pub fn html_report(tree: &DirNode, root: &Path) -> String {
    let filter = ViewFilter::default();
    let types = type_breakdown(tree, root, &filter);
    let categories = category_totals(&types);
    let ages = age_breakdown(tree, root, &filter, filetime_now());
    let files = largest_files(tree, root, 100, &filter);
    let folders = largest_folders(tree, root, 100, &filter);
    let total = tree.size;

    let bar = |p: f64| format!("<span class=bar><span style=\"width:{:.1}%\"></span></span> {:.1}%", p.min(100.0), p);
    let mut h = String::new();
    h.push_str("<!doctype html><html><head><meta charset=utf-8><meta name=viewport content=\"width=device-width,initial-scale=1\">");
    h.push_str(&format!("<title>Disk Usage - {}</title>", html_escape(&root.display().to_string())));
    h.push_str("<style>
:root{--bg:#fff;--fg:#1d1d24;--muted:#6b6b78;--line:#e3e3ea;--bar:#7c5cc4;--track:#ececf3}
@media (prefers-color-scheme:dark){:root{--bg:#1c1b22;--fg:#e8e6f0;--muted:#9a98a8;--line:#34323d;--bar:#9b7fe6;--track:#2e2c37}}
body{font:14px/1.45 'Segoe UI',system-ui,sans-serif;background:var(--bg);color:var(--fg);max-width:1100px;margin:0 auto;padding:24px 16px}
h1{font-size:22px;margin:0 0 4px}h2{font-size:17px;margin:28px 0 8px}.muted{color:var(--muted)}
table{border-collapse:collapse;width:100%}th,td{text-align:left;padding:5px 8px;border-bottom:1px solid var(--line);vertical-align:middle}
th{font-weight:600;color:var(--muted)}td.num{font-variant-numeric:tabular-nums;white-space:nowrap}
.bar{display:inline-block;width:120px;height:9px;background:var(--track);border-radius:3px;vertical-align:middle}
.bar span{display:block;height:100%;background:var(--bar);border-radius:3px}
.path{word-break:break-all}
</style></head><body>");
    h.push_str(&format!(
        "<h1>Disk Usage</h1><div class=path><b>{}</b></div><div class=muted>{} ({} on disk) · {} files · {} folders · exported {}</div>",
        html_escape(&root.display().to_string()),
        format_size(tree.size),
        format_size(tree.allocated),
        tree.file_count,
        tree.dir_count,
        chrono::Local::now().format("%Y-%m-%d %H:%M")
    ));

    h.push_str("<h2>By category</h2><table><tr><th>Category</th><th>Size</th><th>Share</th><th>Files</th></tr>");
    for (cat, size, count) in categories.iter().filter(|c| c.1 > 0) {
        h.push_str(&format!("<tr><td>{cat:?}</td><td class=num>{}</td><td>{}</td><td class=num>{count}</td></tr>", format_size(*size), bar(pct(*size, total))));
    }
    h.push_str("</table><h2>By age (last modified)</h2><table><tr><th>Age</th><th>Size</th><th>Share</th><th>Files</th></tr>");
    for (bucket, size, count) in &ages {
        if *bucket == AgeBucket::Unknown && *count == 0 {
            continue;
        }
        let label = match bucket {
            AgeBucket::UnderMonth => "Under 1 month",
            AgeBucket::Months1To6 => "1 - 6 months",
            AgeBucket::Months6To12 => "6 - 12 months",
            AgeBucket::Years1To3 => "1 - 3 years",
            AgeBucket::Over3Years => "Over 3 years",
            AgeBucket::Unknown => "Unknown",
        };
        h.push_str(&format!("<tr><td>{label}</td><td class=num>{}</td><td>{}</td><td class=num>{count}</td></tr>", format_size(*size), bar(pct(*size, total))));
    }
    h.push_str("</table><h2>Top folders</h2><table><tr><th>Folder</th><th>Size</th><th>Share</th><th>Files</th></tr>");
    for dir in tree.dirs.iter().take(25) {
        h.push_str(&format!(
            "<tr><td class=path>{}</td><td class=num>{}</td><td>{}</td><td class=num>{}</td></tr>",
            html_escape(&dir.name),
            format_size(dir.size),
            bar(pct(dir.size, total)),
            dir.file_count
        ));
    }
    h.push_str("</table><h2>File types</h2><table><tr><th>Type</th><th>Size</th><th>Share</th><th>Files</th></tr>");
    for t in types.iter().take(50) {
        let ext = if t.extension.is_empty() { "(no extension)".to_string() } else { format!(".{}", t.extension) };
        h.push_str(&format!("<tr><td>{}</td><td class=num>{}</td><td>{}</td><td class=num>{}</td></tr>", html_escape(&ext), format_size(t.size), bar(pct(t.size, total)), t.count));
    }
    h.push_str("</table><h2>Largest files</h2><table><tr><th>#</th><th>File</th><th>Size</th><th>Share</th></tr>");
    for (i, f) in files.iter().enumerate() {
        h.push_str(&format!("<tr><td class=num>{}</td><td class=path>{}</td><td class=num>{}</td><td>{}</td></tr>", i + 1, html_escape(&f.path.display().to_string()), format_size(f.size), bar(pct(f.size, total))));
    }
    h.push_str("</table><h2>Largest folders (own files)</h2><table><tr><th>#</th><th>Folder</th><th>Own files</th><th>With subfolders</th></tr>");
    for (i, f) in folders.iter().enumerate() {
        h.push_str(&format!("<tr><td class=num>{}</td><td class=path>{}</td><td class=num>{}</td><td class=num>{}</td></tr>", i + 1, html_escape(&f.path.display().to_string()), format_size(f.own_size), format_size(f.total_size)));
    }
    h.push_str("</table><p class=muted>Made with EdenExplorer</p></body></html>");
    h
}

/// Writes `kind` for `tree` to `dest`.
pub fn export(kind: ExportKind, tree: &DirNode, root: &Path, dest: &Path) -> io::Result<()> {
    let mut w = io::BufWriter::new(std::fs::File::create(dest)?);
    match kind {
        ExportKind::FoldersCsv => write_folders_csv(&mut w, tree, root)?,
        ExportKind::FilesCsv => write_files_csv(&mut w, tree, root)?,
        ExportKind::TypesCsv => write_types_csv(&mut w, &type_breakdown(tree, root, &ViewFilter::default()))?,
        ExportKind::Json => write_json(&mut w, tree, root)?,
        ExportKind::Html => w.write_all(html_report(tree, root).as_bytes())?,
    }
    w.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::disk_usage::FileEntry;

    fn sample() -> DirNode {
        let mut sub = DirNode {
            name: "Sub, \"quoted\"".into(),
            files: vec![FileEntry { name: "b.mp4".into(), size: 300, allocated: 4096, modified: 133_500_000_000_000_000 }],
            ..Default::default()
        };
        sub.recompute_totals();
        let mut root = DirNode {
            name: r"C:\r".into(),
            dirs: vec![sub],
            files: vec![FileEntry { name: "a<b>.txt".into(), size: 10, allocated: 4096, modified: 0 }],
            ..Default::default()
        };
        root.recompute_totals();
        root.sort_children();
        root
    }

    #[test]
    fn csv_quotes_awkward_fields() {
        let mut out = Vec::new();
        write_folders_csv(&mut out, &sample(), Path::new(r"C:\r")).unwrap();
        let text = String::from_utf8(out).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[1], r"C:\r,310,8192,2,1");
        assert_eq!(lines[2], r#""C:\r\Sub, ""quoted""",300,4096,1,0"#);

        let mut out = Vec::new();
        write_files_csv(&mut out, &sample(), Path::new(r"C:\r")).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains(r"C:\r\a<b>.txt,10,4096,"));
        assert!(text.contains("2024-"), "modified date: {text}");
    }

    #[test]
    fn json_round_trips_the_tree() {
        let mut out = Vec::new();
        write_json(&mut out, &sample(), Path::new(r"C:\r")).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(value["root"], r"C:\r");
        assert_eq!(value["tree"]["size"], 310);
        assert_eq!(value["tree"]["dirs"][0]["files"][0]["name"], "b.mp4");
    }

    #[test]
    fn html_report_escapes_names() {
        let html = html_report(&sample(), Path::new(r"C:\r"));
        assert!(html.starts_with("<!doctype html>"));
        assert!(html.contains("a&lt;b&gt;.txt"));
        assert!(!html.contains("a<b>.txt"));
        assert!(html.contains(".mp4"));
    }

    #[test]
    fn summary_lists_the_top_items() {
        let text = summary_text(&sample(), Path::new(r"C:\r"));
        assert!(text.contains("310 B"));
        assert!(text.contains(".mp4"));
        assert!(text.contains(r"C:\r\Sub"));
    }

    #[test]
    fn filetimes_become_dates() {
        assert_eq!(filetime_text(0), "");
        assert_eq!(filetime_text(116_444_736_000_000_000 + 86_400 * 10_000_000), "1970-01-02 00:00");
    }
}
