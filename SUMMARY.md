# EdenExplorer — Project Summary

## What it is
EdenExplorer is an open-source, high-performance file manager for Windows 10/11, built in Rust with [egui](https://github.com/emilk/egui/) (immediate-mode GUI). It's positioned as a fast, modern, FOSS alternative to Windows File Explorer, using direct NT-level filesystem APIs for performance instead of relying on higher-level Windows shell abstractions.

- **License:** MIT
- **Current version:** 0.27.0 (`Cargo.toml`)
- **Status:** Public alpha, actively developed
- **Repo owner:** mtucciarone/EdenExplorer

## Core technology
- **Language:** Rust (edition 2024)
- **GUI:** egui / eframe 0.35, egui_extras, egui-phosphor (icon set)
- **Windows integration:** `windows` crate (0.62) with extensive Win32 feature set (Shell, filesystem, registry, DWM/GDI, portable devices, DirectWrite, globalization, etc.), plus `ntapi` for direct NT-level filesystem access
- **Concurrency:** `rayon` (parallelism), `crossbeam-channel` (async/background communication), `num_cpus`
- **Persistence:** `bincode` / `postcard` (binary settings cache), `serde` / `serde_json`
- **i18n:** `fluent` + `fluent-bundle` + `unic-langid`, with embedded locale assets via `rust-embed` and OS locale detection via `sys-locale`
- **Other:** `image` (thumbnails/icons), `rfd` (native dialogs), `lru` (caching), `chrono`, `dirs`, `rand`

## Project layout (`src/`)
- **`main.rs` / `build.rs`** — entry point and Windows resource/icon build script (`winres`)
- **`core/`** — non-UI logic:
  - `drives.rs` — drive enumeration/detection (including raw/unmounted drives)
  - `fs.rs` — filesystem operations
  - `indexer.rs` — directory scanning/indexing
  - `launch.rs` — launching files/programs
  - `portable.rs` — portable device support (iPhone, Android, etc.)
  - `utils/` — clipboard, colors, dialogs, file helpers, fonts, sorting, tabs, text, thumbnails, widgets
- **`gui/`** — UI layer:
  - `mod.rs`, `theme.rs`, `icons.rs`, `i18n.rs`, `utils.rs`, `dragdrop.rs`
  - `windows/` — application windows/panels: main window (+ impl split), navigation, settings, about, theme customization, drag-and-drop, shell context menu, Windows-specific overrides
  - `windows/containers/` — explorer view, item viewer (+ gallery/navbar/helper variants), sidebar, tabs, tags, topbar, shared structs/enums
- **`locales/`** — translation files (English, Indonesian, Japanese, multiple Chinese variants)
- **`scripts/`, `logo/`** — build/release helper scripts and branding assets
- **`.releaserc.json`** — semantic-release configuration for automated releases
- **`.claude/skills/`** — project skills capturing reusable project knowledge:
  `eden-explorer-ui-test` (build/launch/drive the app for live verification),
  `egui-patterns-reference` (documented egui techniques used here),
  `egui-design-review` (UI/UX consistency checklist),
  `i18n-sync-check` (locale-parity checker script)

## Project documentation
- **`README.md`** — public-facing overview, feature list, roadmap, build/debug instructions, keyboard shortcuts
- **`PRD.md`** — product requirements: positioning, personas, principles, capability inventory, known gaps, planned and proposed enhancements, open questions
- **`CHECKLIST.md`** — phase-by-phase delivery tracking (`Completed` / `In-Progress` / `Pending`)
- **`CLAUDE.md`** — engineering guide: architecture map, conventions, safety rules, recorded mistakes, operational instructions
- **`SUMMARY.md`** — this file
- **`SECURITY.md`** — security policy

## Key features
- Root-level drive view on startup with detailed storage info
- Asynchronous/streaming directory scanning that doesn't block the UI
- Tabbed browsing with pinned tabs, independent loading states, and full navigation history (back/forward/up)
- Real-time fuzzy search/filtering with cached indices
- Sidebar with favorites (drag-and-drop reorderable) and quick access
- Dark/light themes with a full custom color palette editor (hex/RGB/HSL input), layout options (row density, sidebar width), and twelve prebuilt two-color presets (an accent plus a color-theory-contrasted secondary that drives the toolbar icon row, all buttons, and dialog accents); user-created custom themes (save/apply/edit/delete your own accent + secondary pair); nearly every UI surface is genuinely palette-driven and editable - sidebar, status bar, address bar, search box, toolbar, preview pane, and notifications (including semantic green/yellow/red status colors), plus primary-button text and a real disabled-button color pair; consistently-styled dialogs (Checksums, Paste Conflict, Bulk Rename) and a themed Tags settings page; consistent address-bar/search-box borders and padded icon click targets throughout
- Tagging system for files/folders with tag-colored views, plus custom folder colors
- Gallery/thumbnail view (small to extra-large) for media
- Context menu operations (cut/copy/paste/rename/delete), optional native Windows shell context menu integration
- Drag-and-drop both within the app and to/from native Windows (Explorer, Desktop, other apps)
- Paste/drag-and-drop name-collision handling: a themed Replace / Skip Existing / Rename dialog (both flows share the same conflict-check + robocopy pipeline), with safe auto-numbered renaming that never overwrites the original file mid-operation
- Compress to zip: a context-menu entry that archives the selection in the background (named after the single item, or the shared parent folder for a multi-selection; never overwrites an existing zip of the same name)
- Recent locations: a sidebar section tracking the most-recently-visited real folders (capped at 20, excludes anything already in Favorites), recorded from the single choke point every navigation already passes through to load a directory
- Global search (search icon / Ctrl+F in the navigation bar) with a recursive "This folder" / "Everywhere" scope toggle, powered by voidtools Everything (with automatic fallback to a built-in filesystem search when Everything isn't installed/running, also user-selectable in Settings) - results open as a normal tab compatible with every view mode and show each match's source folder; both engines support `*`/`?` wildcard queries, plain keyword matching, and `key:value` filter syntax (`ext:pdf`, `size:>10mb`, `modified:today`, etc.), discoverable via an info icon next to the search box
- Saved Searches: a dedicated sidebar section (separate from Favorites/Tags) for one-click-saved queries + scope, reopened via the same search-tab mechanism a live search uses; capped at 50, renamable/deletable via right-click
- Content search: `content:<word>` filter (folder-scoped only) alongside `ext:`/`size:`/`modified:` - a streamed, text-only line scan for the built-in engine, or Everything's own native `content:` keyword (requires Content Indexing enabled in Everything itself)
- Shared Network device discovery via legacy SMB/NetBIOS browsing, an active subnet sweep, and (new) hand-rolled mDNS + WS-Discovery queries - each candidate device is still verified over SMB before it's shown, so every entry is guaranteed browsable
- Portable device support and raw/unmounted drive (ISO, Linux partition) detection
- Native Recycle Bin support
- Multi-language UI (English, Indonesian, Japanese, Chinese variants) via Fluent i18n
- Persistent settings via an efficient binary cache; column sort/view state persists per directory
- Rich keyboard shortcut set (tab management, navigation, rename, properties, refresh, etc.)
- Six view modes (Details, Detail+Preview, Gallery, Columns, Column+Preview, Preview) with per-directory column layout, fit-to-content, and reordering
- Preview pane for text (with syntax highlighting for code files), Markdown (rendered, including highlighted fenced code blocks and native-rendered Mermaid flowchart/sequence diagrams), images, animated GIFs, SVG (rasterized, including gzip-compressed .svgz), video, audio (waveform + transport controls), PDF/Word/EPUB text extraction, and zip/7z archive listings — with selectable text and right-click copy. Video and audio playback never starts or loops on its own - it waits for the user to press play, and stops at the end
- Delete to Recycle Bin plus an explicit "Delete Permanently" action using the native Windows confirmation
- Explorer-style click-pause-click inline rename (extension excluded from the selection)
- Bulk rename: multi-selection "Rename" opens a live-preview dialog (simple find/replace or regex with capture groups, plus `{name}`/`{ext}`/`{n}`/`{n:03}` placeholders), with cross-item and existing-file collision detection blocking the commit until resolved, applied as one batched native operation
- `shell:` URI / CLSID resolution in the address bar: `shell:ControlPanelFolder`, `shell:Desktop`, `::{GUID}`, etc. resolve through the real Windows shell namespace - filesystem-backed locations navigate in-app, genuinely virtual ones open externally via the shell
- Undo/Redo (Ctrl+Z/Ctrl+Y/Ctrl+Shift+Z) for Rename, Bulk Rename, Move, and Copy - in-memory only, capped at 50 entries. Replace-resolved conflicts are non-destructive (the overwritten item is recycled, not destroyed) and fully undoable/redoable, re-recycling fresh on each redo so a later undo still has something valid to restore
- Checksums: a single-file context-menu entry computing CRC32/MD5/SHA-1/SHA-256 in one background-threaded pass, with per-hash copy buttons and a paste-to-verify Compare field that checks a pasted checksum against all four algorithms at once
- Multi-tagging: an item can belong to more than one tag group at once (the picker is a multi-select toggle list, staying open across picks); a Tags column shows every tag on a row as a colored chip (own smaller font, truncated with an ellipsis if a name doesn't fit) with "+N" overflow; removing a tag from within one specific tag's own view removes it from that tag only, not every tag the item has
- "Open File Location" context menu entry in the Tag view and Search results (both show items from many different real folders) - opens the item's containing folder in a new tab
- Custom Context Menu: export/import just your custom right-click commands as a standalone file (a full settings export already includes them too), and an unquoted-path placeholder (`%L`, alongside the existing always-quoted `%1`/`%V`) for commands that want to add their own quoting or none at all

## Roadmap (from README)
Upcoming alpha features: multi-tag AND/OR filtering in the tag view. Detailed status lives in `CHECKLIST.md`; rationale and backlog ideas in `PRD.md`.

## Distribution
Distributed as a standalone Windows executable (no install) via GitHub Releases at `github.com/mtucciarone/EdenExplorer/releases`, with an automated release workflow (GitHub Actions + semantic-release).
