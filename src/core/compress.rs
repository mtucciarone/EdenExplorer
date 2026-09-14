//! Compress selected files/folders into a single `.zip`, from the context
//! menu's "Compress (zip)" entry. Runs on a background thread (a large
//! folder can take a while to walk and deflate) and reports back through a
//! channel, following the same "start an async job, poll it each frame,
//! finish the notification when it resolves" shape `paste_clipboard_native`/
//! `poll_pending_paste` already use for robocopy jobs - just without the
//! live byte-progress bar, since zip has no equivalent external process to
//! poll.

use crate::core::robocopy::next_available_name;
use crossbeam_channel::Sender;
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

/// Computes where the new zip should be created and what to name it:
/// - One selected item: named after that item (a file's own extension is
///   replaced with `.zip`; a folder keeps its whole name plus `.zip`).
/// - Multiple selected items: named after their shared parent folder.
///
/// Auto-numbered (`name-001.zip`, ...) via the same `next_available_name`
/// helper the paste-conflict modal's "Rename" choice already uses, so an
/// existing file of the same name is never silently overwritten.
pub fn compress_target_path(paths: &[PathBuf]) -> Option<PathBuf> {
    let parent = paths.first()?.parent()?.to_path_buf();

    let base_name = if paths.len() == 1 {
        let path = &paths[0];
        let name = path.file_name()?.to_string_lossy().to_string();
        if path.is_dir() {
            name
        } else {
            Path::new(&name)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or(name)
        }
    } else {
        parent.file_name()?.to_string_lossy().to_string()
    };

    let candidate_name = format!("{base_name}.zip");
    let zip_name = if parent.join(&candidate_name).exists() {
        next_available_name(&parent, &candidate_name)
    } else {
        candidate_name
    };
    Some(parent.join(zip_name))
}

/// Spawns the background thread and sends `Ok(())`/`Err(message)` once the
/// whole archive is written (or a step failed partway through - in which
/// case the partially-written zip file is left in place rather than
/// silently deleted, so the user can inspect what did make it in).
pub fn compress_paths_async(paths: Vec<PathBuf>, dest_zip: PathBuf, tx: Sender<Result<(), String>>) {
    std::thread::spawn(move || {
        let result = compress_paths(&paths, &dest_zip);
        let _ = tx.send(result);
    });
}

fn compress_paths(paths: &[PathBuf], dest_zip: &Path) -> Result<(), String> {
    let file = File::create(dest_zip).map_err(|e| format!("Couldn't create {}: {e}", dest_zip.display()))?;
    let mut writer = ZipWriter::new(BufWriter::new(file));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    for path in paths {
        let Some(name) = path.file_name().map(|n| n.to_string_lossy().to_string()) else {
            continue;
        };
        if path.is_dir() {
            add_dir_recursive(&mut writer, path, &name, options)?;
        } else {
            add_file(&mut writer, path, &name, options)?;
        }
    }

    writer
        .finish()
        .map_err(|e| format!("Failed to finalize archive: {e}"))?;
    Ok(())
}

fn add_file(
    writer: &mut ZipWriter<BufWriter<File>>,
    path: &Path,
    zip_path: &str,
    options: SimpleFileOptions,
) -> Result<(), String> {
    writer
        .start_file(zip_path, options)
        .map_err(|e| format!("Failed to add {zip_path}: {e}"))?;
    let mut source = File::open(path).map_err(|e| format!("Couldn't read {}: {e}", path.display()))?;
    std::io::copy(&mut source, writer).map_err(|e| format!("Failed to write {zip_path}: {e}"))?;
    Ok(())
}

/// Recurses into a real directory, adding an explicit directory entry first
/// (so an empty folder is preserved in the archive) then every child.
/// Symlinks/junctions are added as opaque entries rather than followed,
/// which both avoids an infinite loop on a self-referential link and
/// matches how this codebase's other recursive walks (`search_builtin_async`
/// in `core::fs`) already treat reparse points.
fn add_dir_recursive(
    writer: &mut ZipWriter<BufWriter<File>>,
    dir: &Path,
    zip_prefix: &str,
    options: SimpleFileOptions,
) -> Result<(), String> {
    writer
        .add_directory(format!("{zip_prefix}/"), options)
        .map_err(|e| format!("Failed to add folder {zip_prefix}: {e}"))?;

    let entries = std::fs::read_dir(dir).map_err(|e| format!("Couldn't read {}: {e}", dir.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let zip_path = format!("{zip_prefix}/{name}");

        if path.is_symlink() {
            continue;
        }
        if path.is_dir() {
            add_dir_recursive(writer, &path, &zip_path, options)?;
        } else {
            add_file(writer, &path, &zip_path, options)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("eden_compress_test_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn single_file_names_zip_after_stem_replacing_extension() {
        let dir = scratch_dir("single_file");
        let file = dir.join("report.pdf");
        std::fs::write(&file, b"x").unwrap();

        let target = compress_target_path(&[file]).unwrap();
        assert_eq!(target, dir.join("report.zip"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn single_folder_keeps_its_whole_name() {
        let dir = scratch_dir("single_folder");
        let sub = dir.join("Photos");
        std::fs::create_dir_all(&sub).unwrap();

        let target = compress_target_path(&[sub]).unwrap();
        assert_eq!(target, dir.join("Photos.zip"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn multiple_items_are_named_after_their_shared_parent() {
        let dir = scratch_dir("multi");
        let a = dir.join("a.txt");
        let b = dir.join("b.txt");
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&b, b"b").unwrap();

        let target = compress_target_path(&[a, b]).unwrap();
        let expected_name = format!("{}.zip", dir.file_name().unwrap().to_string_lossy());
        assert_eq!(target, dir.join(expected_name));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn existing_zip_name_gets_auto_numbered_instead_of_overwritten() {
        let dir = scratch_dir("collision");
        let file = dir.join("notes.txt");
        std::fs::write(&file, b"x").unwrap();
        std::fs::write(dir.join("notes.zip"), b"already here").unwrap();

        let target = compress_target_path(&[file]).unwrap();
        assert_ne!(target, dir.join("notes.zip"));
        assert!(target.file_name().unwrap().to_string_lossy().starts_with("notes-"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn compress_paths_writes_a_real_readable_zip_with_a_nested_folder() {
        let dir = scratch_dir("roundtrip");
        std::fs::write(dir.join("top.txt"), b"top-level").unwrap();
        let sub = dir.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("inner.txt"), b"nested").unwrap();

        let dest = dir.join("out.zip");
        let paths = vec![dir.join("top.txt"), sub.clone()];
        compress_paths(&paths, &dest).expect("compress should succeed");

        let file = std::fs::File::open(&dest).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect();
        assert!(names.contains(&"top.txt".to_string()));
        assert!(names.iter().any(|n| n == "sub/" || n.starts_with("sub/")));
        assert!(names.contains(&"sub/inner.txt".to_string()));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
