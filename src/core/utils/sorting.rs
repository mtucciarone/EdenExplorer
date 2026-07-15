use crate::core::fs::FileItem;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering::{Greater, Less};

#[derive(PartialEq, Clone, Copy, Debug, Serialize, Deserialize)]
pub enum SortColumn {
    Name,
    Size,
    Modified,
    Created,
    Type,
}

pub fn sort_files(files: &mut Vec<FileItem>, column: SortColumn, ascending: bool) {
    files.sort_by(|a, b| {
        if a.is_dir != b.is_dir {
            return if a.is_dir { Less } else { Greater };
        }

        let ord = match column {
            SortColumn::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SortColumn::Size => a.file_size.unwrap_or(0).cmp(&b.file_size.unwrap_or(0)),
            SortColumn::Modified => a.modified_time_raw.cmp(&b.modified_time_raw),
            SortColumn::Created => a.created_time_raw.cmp(&b.created_time_raw),
            SortColumn::Type => match (a.is_dir, b.is_dir) {
                (true, false) => Less,
                (false, true) => Greater,
                (true, true) => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                (false, false) => {
                    let a_ext = a
                        .path
                        .extension()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_lowercase();
                    let b_ext = b
                        .path
                        .extension()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_lowercase();
                    a_ext.cmp(&b_ext)
                }
            },
        };

        if ascending { ord } else { ord.reverse() }
    });
}
