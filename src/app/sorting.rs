use crate::state::FileItem;

#[derive(PartialEq, Clone, Copy)]
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
            return if a.is_dir {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            };
        }

        let ord = match column {
            SortColumn::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SortColumn::Size => a.file_size.unwrap_or(0).cmp(&b.file_size.unwrap_or(0)),
            SortColumn::Modified => a.modified_time.cmp(&b.modified_time),
            SortColumn::Created => a.created_time.cmp(&b.created_time),
            SortColumn::Type => {
                // Sort by folder/file first, then by file extension
                match (a.is_dir, b.is_dir) {
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                    (true, true) => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                    (false, false) => {
                        // For files, sort by extension
                        let a_ext = a.path.extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
                        let b_ext = b.path.extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
                        a_ext.cmp(&b_ext)
                    }
                }
            }
        };

        if ascending {
            ord
        } else {
            ord.reverse()
        }
    });
}
