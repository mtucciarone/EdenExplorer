use crate::core::fs::FileItem;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering::{Equal, Greater, Less};

#[derive(PartialEq, Clone, Copy, Debug, Serialize, Deserialize)]
pub enum SortColumn {
    Name,
    Size,
    Modified,
    Created,
    Type,
    Deleted,
    OriginalDirectory,
}

#[derive(PartialEq, Clone, Copy, Debug, Serialize, Deserialize)]
pub struct SortKey {
    pub column: SortColumn,
    pub ascending: bool,
}

pub fn sort_files_by_keys(files: &mut Vec<FileItem>, keys: &[SortKey]) {
    files.sort_by(|a, b| {
        if a.is_dir != b.is_dir {
            return if a.is_dir { Less } else { Greater };
        }

        for key in keys {
            let ord = match key.column {
                SortColumn::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                SortColumn::Size => a.file_size.unwrap_or(0).cmp(&b.file_size.unwrap_or(0)),
                SortColumn::Modified => a.modified_time_raw.cmp(&b.modified_time_raw),
                SortColumn::Created => a.created_time_raw.cmp(&b.created_time_raw),
                SortColumn::Deleted => a.deleted_time_raw.cmp(&b.deleted_time_raw),
                SortColumn::OriginalDirectory => a.original_directory.cmp(&b.original_directory),
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

            if ord != Equal {
                return if key.ascending { ord } else { ord.reverse() };
            }
        }

        Equal
    });
}
