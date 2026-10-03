# EdenExplorer Roadmap

Ideas for future features, gathered from other file managers (Mac Finder, ForkLift, Path Finder, Nautilus, Dolphin, Thunar, File Pilot, Files App, FileCommander, Directory Opus, QTTabBar, Total Commander, TeraCopy) and disk space analyzers (WinDirStat, RidNacs, WizTree, TreeSize, DaisyDisk, Filelight).

Items marked [x] have been implemented (see the README changelog); the rest aren't scheduled yet. Items already in the app (tabs, split view, tags, bulk rename, previews, checksums, zip, Everything search, custom context menu, undo/redo, Performance panel, ...) are left out.

**Effort:** **S** = a day or two · **M** = about a week · **L** = a larger project

## Suggested Priorities

1. ~~**Disk Usage Analyzer**~~ - done: every item in section 1 is implemented.
2. ~~**Quick Look (Space)** and a **command palette**~~ - done.
3. ~~**Archive extraction** and **queued transfers with verify-after-copy**~~ - done, along with batch conflict rules.
4. ~~**Folder compare/sync**~~ - deferred with the other multi-pane work (see Deferred below); editable shortcuts are already done.

---

## 1. Disk Usage Analyzer

*Inspired by WinDirStat, RidNacs, WizTree, TreeSize, DaisyDisk.*

### Entry points
- [x] **"Analyze Disk Usage…"** in the right-click menu for folders and drives (including drives in This PC and the sidebar), opening a dashboard dialog or a dedicated tab (L)
- [x] **Fast whole-drive scan** that reads the NTFS Master File Table directly, like WizTree; needs admin rights, so it asks for them with one UAC prompt (running only the MFT read elevated) and falls back to the existing NT scan without them (L)
- [x] Background scanning with progress, Cancel, and "Rescan This Branch" (M)

### Dashboard views
- [x] **Treemap** (WinDirStat-style cushion shading): click a block to select the file, double-click to zoom in, breadcrumbs to zoom out (L)
- [x] **Sunburst ring chart** (DaisyDisk, Filelight, Baobab) as an alternative to the treemap (M)
- [x] **Tree with percentage bars** (RidNacs, TreeSize): name, size, % of parent, file count, colored bar, expandable rows (M)
- [x] **File type breakdown** (WinDirStat extension list): size and count per extension, colored to match the treemap; selecting a type highlights it in the treemap (M)
- [x] **Top 100 largest files**, with Reveal, Delete, and Move actions (S)
- [x] **Top 100 largest folders** (by their own files, not counting subfolders), with the same actions (S)
- [x] **Age chart**: how much space is in files not touched for 1 month, 1 year, 3+ years (S)
- [x] **Drive summary**: used/free space, cluster size, and wasted "slack" space (S)

### Actions and extras
- [x] **Cleanup actions** from the dashboard (Recycle, Delete Permanently, Move To, Compress, Open in Tab), going through the existing notification and undo system (M)
- [x] **Duplicate finder**: match by size, then partial hash, then full hash (reusing the checksum code); groups with "keep newest/oldest" helpers (M)
- [x] **Snapshots and compare**: save a scan and later show what grew or shrank since then (M)
- [x] **Export** results as CSV, HTML report, or JSON, plus "Copy Summary" like the benchmark's Copy Results (S)
- [x] **Filters**: exclude folders, only files over X MB, only one file type (S)
- [x] **Drive benchmark**: sequential and random read/write speed (like CrystalDiskMark), next to the folder-listing benchmark in the Performance panel (M)
- [x] **Storage Sense-style cleanup shortcuts**: measure and clean Temp, Windows Update cache, browser caches, the Recycle Bin, and old Downloads in one click (M)

## 2. Navigation and Layout

- [x] **Quick Look on Space** (Finder, Files App, QTTabBar): full-size preview pop-up using the existing preview engine, arrow keys move between files (S–M)
- [x] **Command palette** on Ctrl+Shift+P (File Pilot, Files App, VS Code): every action and setting searchable (M)
- [ ] **Go To Folder with fuzzy matching** on recently visited folders (Directory Opus, File Pilot, zoxide) (S)
- [ ] **Tree view in the sidebar** (Dolphin, Directory Opus, Windows Explorer) (M)
- [ ] **Flat view**: every file in all subfolders as one list (Directory Opus) (S)
- [x] **Per-folder view memory**: each folder remembers its own view mode, sort, and columns (Finder, Dolphin, Directory Opus) (S)
- [ ] **Filter bar options**: wildcards, regex, "only images/docs" chips, hide matches (Dolphin, Directory Opus) (S)
- [ ] **Spring-loaded folders**: hovering over a folder while dragging opens it (Finder) (S)
- [ ] **More sidebar places**: pinned network locations, cloud folders, WSL distros, mounted ISOs (Nautilus, Dolphin) (S)
- [ ] **Workspaces / layouts**: save a whole window (tabs, split, sidebar state) and restore it, one step beyond Tab Groups (Directory Opus "Layouts") (M)
- [ ] **Breadcrumb dropdowns** on the `>` arrows listing sibling folders (Explorer, Path Finder) (S)
- [x] **Customizable toolbar**: add, remove, and reorder buttons (Directory Opus, QTTabBar) (M)

## 3. File Operations

- [x] **Queued transfers**: run copies to the same disk one after another instead of in parallel, with reorder and Pause All (TeraCopy, FileCommander, Directory Opus) (M)
- [x] **Verify after copy** using the existing checksums (TeraCopy) (S)
- [x] **Batch conflict rules**: Apply To All, Keep Both, Replace Only If Newer/Larger (M)
- [ ] **Copy To / Move To** with a folder picker and recent destinations (Explorer, Path Finder) (S)
- [x] **New File from templates**: .txt, .md, .docx, .xlsx, or a user templates folder (Nautilus, Dolphin, Files App) (S)
- [x] **Extract archives** (zip, 7z, tar, gz, rar via 7-Zip): Extract Here, Extract to Folder, and browsing into an archive like a folder (Files App, Dolphin, Directory Opus) (M–L)
- [ ] **Split and join large files** (Total Commander) (S)
- [ ] **Secure delete / shred** (FileCommander) (S)
- [ ] **Symbolic links and junctions**: create them and show their targets (Link Shell Extension, Directory Opus) (S)
- [ ] **Batch attributes and timestamps**: set read-only/hidden and modified/created dates for many files (Directory Opus, Total Commander) (S)
- [ ] **Bulk rename upgrades**: EXIF date, audio tags, and padded counters as tokens; regex preview; saved presets (Directory Opus, Advanced Renamer) (M)
- [ ] **Image tools** in the right-click menu: resize, convert, rotate, strip EXIF (Files App, Dolphin, PowerToys) (M)
- [ ] **Undo for Delete**: restore the last deletion from the Recycle Bin (S)

## 4. Metadata and Search

- [ ] **Extra columns**: dimensions, duration, bitrate, EXIF camera/date, author, page count (Directory Opus, Dolphin, Finder) (M)
- [ ] **Git integration**: branch in the status bar and per-file badges for modified/untracked files (Files App, Dolphin plugin) (M)
- [ ] **Smart folders**: saved searches with rules such as "tag = Work AND modified in the last 7 days" that behave like folders, building on Saved Searches (Finder, Path Finder) (M)
- [ ] **Search results**: sort by relevance and path, and "search within results" (S)
- [ ] **File comments / notes** (Finder, Directory Opus) (S)
- [ ] **Portable tags**: carry tags to another PC via sidecar files or NTFS alternate data streams (S)

## 5. Integration and Power Users

- [ ] **Built-in terminal pane** that follows the current folder (Dolphin F4, Path Finder, Files App) (M)
- [ ] **Open With…** with recommended apps and "always use" (S)
- [ ] **Cloud status badges** for OneDrive, Dropbox, and Google Drive (synced / online-only), plus Free Up Space (Files App) (M)
- [ ] **Remote locations**: FTP, SFTP, WebDAV, and S3 you can browse like folders (ForkLift, FileCommander, Directory Opus) (L)
- [ ] **WSL filesystem browsing** (`\\wsl$`) in the sidebar (S)
- [ ] **Scripting and macros**: user scripts (Rhai or Lua) with access to the selection, bound to buttons or shortcuts (Directory Opus, Total Commander) (L)
- [x] **Editable keyboard shortcuts**: make the read-only Settings > Shortcuts page remappable (Directory Opus, File Pilot) (M)
- [x] **Portable mode**: keep settings next to the .exe (Total Commander) (S)
- [x] **Persistent folder size cache** so large folders show sizes instantly on the next visit (S)
- [ ] **Session restore after a crash or restart**, including split view and scroll position (S)

## 6. Quality of Life

- [x] **Hover preview tooltips** for images and videos (QTTabBar) (S)
- [x] **Checkbox selection mode, Invert Selection, and Select by Pattern** (`*.jpg`) (Explorer, Directory Opus) (S)
- [x] **Status bar extras**: selection size, free space, active filter indicator (S)
- [ ] **Drive health**: SMART status and temperature in This PC (M)
- [ ] **Accessibility**: high-contrast theme, UI scaling slider, screen-reader labels (M)
- [ ] **Update check** against GitHub Releases (notify only) (S)

## Deferred

Kept out of the plan for now. The existing two-pane split view stays as it is.

### Multi-pane work (to revisit later)

More panes than today's two need the window's chrome rethought first - with every pane keeping its own address bar and toolbar, four panes leave each one about 800×450 px with only ~15 rows visible. When revisited, the plan is one shared toolbar acting on the focused pane, a slim one-row breadcrumb header per pane, a layout picker (1, 2 side by side, 2 stacked, 3, 2×2), and a minimum pane size. Everything that builds on it waits with it:

- **Up to 4 panes** with layouts and the slim per-pane header (Directory Opus, Total Commander) (M–L)
- **Synchronized browsing**: entering a subfolder in one pane enters the same subfolder in the others, for comparing two folders (Directory Opus, Total Commander) (S, after the panes)
- **Folder compare**: highlight files that differ, are newer, or exist on only one side, then sync left to right (Directory Opus, FreeFileSync) (L)

### Changes to Windows itself

These change Windows rather than the app, and a mistake could leave the system in a bad state.

- **Set as default file manager** so Win+E and folder opens use EdenExplorer (Files App, Directory Opus) (M) - it means rewriting the shell's folder-open registry keys; if that goes wrong (or the app is moved or deleted) folders stop opening from the desktop, taskbar, and other apps. Not planned.
