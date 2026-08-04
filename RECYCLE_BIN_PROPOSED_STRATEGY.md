# Recycle Bin Proposed Strategy

- Treat Recycle Bin as a virtual shell location, not as a normal `PathBuf`.
- Add a dedicated navigation variant so `This PC`, filesystem folders, and Recycle Bin are handled separately.
- Enumerate Recycle Bin through shell COM APIs, using `SHCreateItemFromParsingName` and shell item enumeration instead of `scan_dir_async`.
- Convert shell items into `FileItem` records with shell display names, icons, timestamps, original locations, and other available metadata.
- Special-case actions inside Recycle Bin:
  - restore selected items
  - permanently delete selected items
  - properties
- Disable filesystem-only operations there unless they make sense for shell items.
- Keep the sidebar shortcut as a shell launcher only until in-app shell browsing is implemented.
