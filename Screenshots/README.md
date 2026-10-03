# EdenExplorer Screenshots

All screenshots were taken with sample data (the `Eden Demo` folder: photos, a small website project, a Rust project, invoices, music, a font, and an archive). The Performance panel shots use `C:\Windows\System32` so the benchmark has a few hundred entries to list. Every screenshot shows the current layout: the address bar on its own row above the toolbar, and the status bar with selection size, free space, and the active filter.

## Walkthrough

![EdenExplorer walkthrough](eden-explorer-walkthrough.gif)

Opening the website project in Details + Preview and previewing a Markdown file with a Mermaid diagram, an SVG, and a Python file, jumping back with the address bar breadcrumbs, searching for "invoice", browsing Photos in Gallery and Details + Preview, then opening Settings > Toolbar.

## Analyze Disk Usage

![Analyze Disk Usage](disk-usage.gif)

Right-click the background of a folder > Analyze Disk Usage…, then a tour of the dashboard's tabs: Overview, the Folder Tree with share-of-parent bars, the cushion-shaded Treemap (hovering a block shows its path and size), the Sunburst, File Types, then Duplicates (Find Duplicates, then Keep Oldest marks the extra copies) and Clean Up measuring temp files, caches, the Recycle Bin, and old Downloads.

## Performance Panel Benchmarks

![Performance panel benchmark](performance-benchmark.gif)

Pressing Ctrl+K to open the panel, choosing 3 runs and clicking Benchmark This Folder (each listing method timed, the fastest highlighted), then switching to Benchmark This Drive and running it with a 64 MB test file: sequential and random 4K read/write, phase by phase, ending in MB/s and IOPS bars (recorded under Wine, which can't bypass its cache, so the numbers are cache speed).

## Animated GIF Preview Controls

![Animated GIF preview controls](gif-preview-controls.gif)

Playing, pausing, stepping forward and back a frame, dragging the seek bar, stopping (back to frame 1), and playing again.

## Selection Tools And New File Templates

![Selection tools and New File templates](selection-and-templates.gif)

Select By Pattern (Ctrl+Shift+S) selecting `*.docx; *.txt`, Invert Selection (Ctrl+I), then right-click > New File > Excel Workbook (.xlsx), which goes straight into rename.

## Quick Look

![Quick Look](quick-look.gif)

Selecting a photo and pressing Space opens Quick Look over the window; ↓ and → step through the folder (an animated GIF plays with its controls), Space closes it, then Quick Look on a folder shows its item count and Esc closes it.

## Command Palette

![Command palette](command-palette.gif)

Ctrl+Shift+P, then typing `pho` jumps to the Photos folder, `gal` switches to the Gallery view, `det` back to Details, and `setbeh` opens Settings on the Behavior page.

## Archives

![Extracting and browsing archives](archives.gif)

Right-click a zip > Extract To "Holiday Photos\\" (progress in the notifications panel, then the new folder is selected), then double-click the zip to browse it like a folder: into Beach, Gallery thumbnails straight from the archive, Quick Look on a photo, and back out with the breadcrumbs.

## Archive Previews With VS Code File Icons

![Archive preview with file icons](archive-preview.gif)

Selecting a zip shows its contents in the preview pane as a tree with Material Icon Theme icons (as in VS Code): collapsing src and node_modules, Collapse All, opening the top folder, Expand All, then the same for a tar.gz, in the dark and light themes.

## Queued And Verified Copies

![Queued and verified copies](transfers.gif)

Copying a 180 MB folder to Documents and then to Desktop: the second copy waits as "Queued #1" (with sooner/later/cancel buttons and Pause All in the header) until the first finishes, and with Verify Copies on each one ends with "Verified 3 files".

## Features

### Details view - tagged folders are tinted by tag color, with a Tags column showing every tag as a chip

![Details view - tagged folders are tinted by tag color, with a Tags column showing every tag as a chip](01-details-view-with-tags.png)

### Gallery view with Large thumbnails

![Gallery view with Large thumbnails](02-gallery-view.png)

### Columns view - a Finder-style column browser

![Columns view - a Finder-style column browser](03-columns-view.png)

### Columns + Preview - drill down through folders with a live preview of the selected file

![Columns + Preview - drill down through folders with a live preview of the selected file](04-columns-plus-preview.png)

### Details + Preview - image preview

![Details + Preview - image preview](05-details-plus-preview-image.png)

### Preview pane Details tab - size, dimensions, created/modified dates

![Preview pane Details tab - size, dimensions, created/modified dates](06-preview-details-tab.png)

### Rendered Markdown preview with task lists, tables, and a Mermaid flowchart

![Rendered Markdown preview with task lists, tables, and a Mermaid flowchart](07-markdown-mermaid-preview.png)

### Syntax-highlighted code preview

![Syntax-highlighted code preview](08-syntax-highlighting.png)

### SVG preview

![SVG preview](09-svg-preview.png)

### Font file preview with sample text at several sizes

![Font file preview with sample text at several sizes](10-font-preview.png)

### Zip archive contents preview

![Zip archive contents preview](11-archive-preview.png)

### Find in preview - highlights matches with next/previous navigation

![Find in preview - highlights matches with next/previous navigation](12-find-in-preview.png)

### Split-pane view

![Split-pane view](13-split-view.png)

### File/folder right-click menu

![File/folder right-click menu](14-context-menu.png)

### Send To submenu plus Custom Context Menu commands in the right-click menu

![Send To submenu plus Custom Context Menu commands in the right-click menu](15-send-to-menu.png)

### Tab right-click menu - Pin, Favorites, Save Search, and Close / Close Other / Close Tabs To The Left / Close Tabs To The Right

![Tab right-click menu - Pin, Favorites, Save Search, and Close / Close Other / Close Tabs To The Left / Close Tabs To The Right](16-tab-context-menu.png)

### "+" button menu - open or replace your tabs with a saved Tab Group

!["+" button menu - open or replace your tabs with a saved Tab Group](17-tab-groups-menu.png)

### Multi-row tab strip when tabs no longer fit on one row

![Multi-row tab strip when tabs no longer fit on one row](18-multi-row-tabs.png)

### Multi-tagging - add an item to several tag groups at once

![Multi-tagging - add an item to several tag groups at once](19-multi-tagging.png)

### Tag color picker with hex, RGB, and HSL

![Tag color picker with hex, RGB, and HSL](20-tag-color-picker.png)

### Tag view - every item carrying a tag, from the sidebar's Tags section

![Tag view - every item carrying a tag, from the sidebar's Tags section](21-tag-view.png)

### Global search results tab (Saved Searches appear in the sidebar)

![Global search results tab (Saved Searches appear in the sidebar)](22-search-results.png)

### Bulk Rename in Regex mode with a live preview

![Bulk Rename in Regex mode with a live preview](23-bulk-rename.png)

### Checksums dialog verifying a pasted MD5

![Checksums dialog verifying a pasted MD5](24-checksums.png)

### Paste conflict dialog - Replace / Skip Existing / Rename

![Paste conflict dialog - Replace / Skip Existing / Rename](25-paste-conflict.png)

### Notification panel showing a completed operation

![Notification panel showing a completed operation](26-notifications.png)

### This PC with drive usage

![This PC with drive usage](27-this-pc.png)

### Light theme

![Light theme](28-light-theme.png)

### Settings - General, including Address Bar On Its Own Row and the Sidebar Sections card

![Settings - General](29-settings-general.png)

### Settings - Behavior

![Settings - Behavior](30-settings-behavior.png)

### Settings - Startup & Window

![Settings - Startup & Window](31-settings-startup-window.png)

### Settings - Appearance, with prebuilt themes, color schemes, custom themes, and Live Preview

![Settings - Appearance, with prebuilt themes, color schemes, custom themes, and Live Preview](32-settings-appearance.png)

### Settings - Favorites

![Settings - Favorites](33-settings-favorites.png)

### Settings - Context Menu Order

![Settings - Context Menu Order](34-settings-context-menu-order.png)

### Settings - Custom Context Menu

![Settings - Custom Context Menu](35-settings-custom-context-menu.png)

### Settings - Send To

![Settings - Send To](36-settings-send-to.png)

### Settings - Tab Groups

![Settings - Tab Groups](37-settings-tab-groups.png)

### Settings - Tags

![Settings - Tags](38-settings-tags.png)

### Settings - Shortcuts, where most shortcuts can be changed: + adds a key combination, × removes one, and locked rows are fixed

![Settings - Shortcuts](39-settings-shortcuts.png)

### Settings - Advanced, with Reset Settings, Export/Import Settings, the Reset Data card (now including Folder Views and Folder Size Cache), Portable Mode, and Show Performance Panel

![Settings - Advanced](40-settings-advanced.png)

### Settings - General > Sidebar Sections, with Saved Searches, Recent, and Shared Network hidden from the sidebar

![Sidebar sections hidden](41-sidebar-sections-hidden.png)

### Reset Data confirmation - every reset explains exactly what it removes before anything is deleted

![Reset Data confirmation](42-reset-data-confirm.png)

### Notification panel after deletes - answering No to the delete confirmation (Delete or Shift+Del) is reported as Cancelled and leaves the item and its tags untouched; answering Yes shows Completed

![Delete cancelled and completed in the notification panel](43-delete-cancelled-notifications.png)

### Performance panel (Ctrl+K) - live folder load time and items/sec, folder size scan time, frame time, FPS, and memory for the current folder

![Performance panel](44-performance-panel.png)

### Benchmark This Folder - min/avg/max and items/sec for the app's NtQueryDirectoryFile listing, Rust's read_dir, and read_dir with per-file metadata, with Copy Results

![Benchmark This Folder results](45-performance-benchmark.png)

### Settings - Shortcuts, the Window group with the Show/Hide Performance Panel shortcut

![Settings - Shortcuts, Performance Panel shortcut](46-settings-shortcuts-performance.png)

### Animated GIF preview with media controls - seek bar, Play/Pause, Stop, Previous/Next Frame, current/total time, and the frame number

![Animated GIF preview controls](47-gif-preview-controls.png)

### Status bar - free space on the drive, and the active type-to-filter ("new", 2 of 9 items) with a button to clear it

![Status bar with filter and free space](48-status-bar-filter-free-space.png)

### Right-click > New File - built-in Text, Markdown, Word, and Excel templates, plus Open Templates Folder (files in that folder are listed too); also Select All, Invert Selection, and Select By Pattern

![New File templates](49-new-file-templates.png)

### Select By Pattern (Ctrl+Shift+S) - wildcard patterns with a live match count

![Select By Pattern](50-select-by-pattern.png)

### Gallery selection checkboxes - click a tile's checkbox to add or remove it without Ctrl

![Gallery selection checkboxes](51-gallery-selection-checkboxes.png)

### Hover preview - hovering an image or video in Details view shows its thumbnail

![Hover preview](52-hover-preview.png)

### View menu - Reset Folder View and Use As Default View for the per-folder view memory

![View menu folder view options](53-view-menu-folder-view.png)

### Settings - Toolbar, with a live preview; here Invert Selection, Select By Pattern, and Settings were added

![Settings - Toolbar](54-settings-toolbar.png)

### Settings - Shortcuts after adding Ctrl+E to New Tab (customized rows get a • and a reset button)

![Settings - Shortcuts editing](55-settings-shortcuts-editing.png)

### Settings - Behavior, with Remember View Per Folder, Remember Folder Sizes, Hover Previews, and the New File Templates Folder

![Settings - Behavior](56-settings-behavior.png)

### Analyze Disk Usage… - where the space in a folder goes: folders and files largest first, each with its share of the parent folder, size on disk, and file/folder counts; Rescan This Branch re-reads the selected folder

![Analyze Disk Usage](57-disk-usage.png)

### Disk Usage scanning in the background, with the files, size, and folders found so far, the folder being read, and Cancel

![Disk Usage scan in progress](58-disk-usage-scanning.png)

### Analyze Disk Usage… in a folder's background right-click menu (it's also on folders, drives in This PC, and drives and favorites in the sidebar)

![Analyze Disk Usage menu](59-disk-usage-menu.png)

### Disk Usage > Largest Files - the 100 biggest files in the analyzed folder, ranked, with their folder, share of the total, and size on disk; two files selected with Ctrl+Click

![Disk Usage - Largest Files](60-disk-usage-largest-files.png)

### Largest Files right-click menu - Show In Folder, Move To…, Delete, Delete Permanently, and Copy Path

![Largest Files menu](61-disk-usage-largest-menu.png)

### Disk Usage > Largest Folders - folders ranked by the files directly inside them (Vacation 2025 owns 160 MB, 280 MB with its Raw Clips subfolder, which is listed on its own), with share of the total, file count, and size including subfolders

![Disk Usage - Largest Folders](62-disk-usage-largest-folders.png)

### Largest Folders right-click menu - Open In New Tab, Show In Folder, Rescan This Branch, Move To…, Delete, Delete Permanently, and Copy Path

![Largest Folders menu](63-disk-usage-largest-folders-menu.png)

### Disk Usage > Overview - drive summary with cluster slack, space by category (click one to see it in File Types), and space by age

![Disk Usage - Overview](71-disk-usage-overview.png)

### Disk Usage > File Types - every extension with its category, size, share, count, size on disk, and largest file; the colors match the treemap and sunburst

![Disk Usage - File Types](72-disk-usage-file-types.png)

### Disk Usage > Treemap - cushion-shaded blocks colored by file type, with breadcrumbs, Zoom In/Out, and Highlight

![Disk Usage - Treemap](65-disk-usage-treemap.png)

### Disk Usage > Sunburst - rings for each folder level, colored by top-level folder; the center zooms back out

![Disk Usage - Sunburst](64-disk-usage-sunburst.png)

### Folder Tree right-click menu with the cleanup actions every view shares - Move To…, Compress To ZIP, Delete, Delete Permanently - and Move To… and Delete in the toolbar

![Disk Usage cleanup menu](67-disk-usage-cleanup-menu.png)

### Snapshots > Compare With - the Changes tab after adding an ISO, removing a zip, and growing a video: folders that grew, with before, now, and difference

![Disk Usage - Changes](66-disk-usage-changes.png)

### Disk Usage > Duplicates - groups of identical files with how much each group can free; Keep Newest / Keep Oldest mark the extra copies

![Disk Usage - Duplicates](68-disk-usage-duplicates.png)

### Disk Usage > Clean Up - temp files, Windows Update cache, browser caches, Recycle Bin, and old Downloads, each measured with a Clean button

![Disk Usage - Clean Up](69-disk-usage-cleanup.png)

### Performance panel > Benchmark This Drive - sequential and random 4K read/write speed and IOPS (this run is under Wine, which can't bypass its cache, so the numbers are cache speed)

![Benchmark This Drive](70-drive-benchmark.png)

### Quick Look (Space) - a large preview of the selected file, with its size, date, position in the folder, previous/next, and Open

![Quick Look](73-quick-look.png)

### Command palette (Ctrl+Shift+P) - typing `set` lists the Settings pages first; commands show their shortcuts, folders their paths

![Command palette](74-command-palette.png)

### Right-click a zip - Extract Here, Extract To "Holiday Photos\\", and Extract To…

![Extract menu](75-extract-menu.png)

### Browsing inside a zip in Gallery view - thumbnails, folder sizes, and breadcrumbs through the archive

![Browsing an archive](76-archive-gallery.png)

### Right-click items inside an archive - Open, Extract Next To The Archive, Extract To…, and Copy Path

![Archive item menu](77-archive-menu.png)

### Paste conflict dialog - Keep Both, Apply To All, and Replace Only If The Pasted One Is Newer / Larger

![Paste conflict rules](78-paste-conflict.png)

### Notifications - a second copy to the same disk queued behind the first, with reorder, cancel, and Pause All

![Transfer queue](79-transfer-queue.png)

### Notifications - both copies finished and verified by checksum

![Verified copies](80-verified.png)

### Preview pane - a zip's contents in a bordered panel, as a VS Code-style tree with Material Icon Theme icons for each file type and named folder

![Archive preview with file icons](81-archive-preview-icons.png)

### Settings > Appearance > Preview Frame - border thickness, border color (theme or custom), and corner radius, with the calculated padding and inner corner radius

![Preview frame settings](82-preview-frame-settings.png)

### A custom preview frame - 2 px purple border with 16 px corners around an archive preview

![Custom preview frame](83-preview-frame-custom.png)

### Breadcrumb folder menu - the arrow after Eden Demo lists its folders, with Photos (on the current path) highlighted

![Breadcrumb folder menu](84-breadcrumb-menu.png)

### Terminal pane (Ctrl+`) - PowerShell 7 and Ubuntu (WSL) session tabs docked under the file view, with colors, wide characters, and the cursor (sample output: under Wine the shells can't take input, so this text was fed to the terminal directly)

![Terminal pane](85-terminal-pane.png)

### The terminal pane's + menu - every installed shell (Command Prompt, Windows PowerShell, PowerShell 7, Git Bash, each WSL distribution) and Default Shell

![Terminal shells](86-terminal-shells.png)
