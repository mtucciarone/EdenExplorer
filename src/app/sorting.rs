use crate::state::FileItem;

#[derive(PartialEq, Clone, Copy)]
pub enum SortColumn {
    Name,
    Size,
    Modified,
}

pub fn sort_files(
    files: &mut Vec<FileItem>,
    column: SortColumn,
    ascending: bool,
) {
    files.sort_by(|a, b| {
        if a.is_dir != b.is_dir {
            return if a.is_dir {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            };
        }

        let ord = match column {
            SortColumn::Name => {
                a.name.to_lowercase().cmp(&b.name.to_lowercase())
            }
            SortColumn::Size => {
                a.file_size.unwrap_or(0).cmp(&b.file_size.unwrap_or(0))
            }
            SortColumn::Modified => {
                a.modified_time.cmp(&b.modified_time)
            }
        };

        if ascending { ord } else { ord.reverse() }
    });
}