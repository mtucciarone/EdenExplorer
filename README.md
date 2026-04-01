<table>
  <tr>
    <td width="60%" valign="middle">

# EdenExplorer
**Blazing-fast, open-source file manager for Windows 11.**

EdenExplorer is a next-generation file explorer built with Rust and egui,
focused on performance, efficiency, and modern workflows.

A fast, open-source alternative to Windows File Explorer.
<p>
  <img src="https://img.shields.io/github/v/release/mtucciarone/EdenExplorer?style=for-the-badge&color=5F4B87&labelColor=14161A" />
  <img src="https://img.shields.io/github/license/mtucciarone/EdenExplorer?style=for-the-badge&color=5F4B87&labelColor=14161A" />
  <img src="https://img.shields.io/github/actions/workflow/status/mtucciarone/EdenExplorer/release.yml?style=for-the-badge&color=5F4B87&labelColor=14161A" />
  <img src="https://img.shields.io/github/stars/mtucciarone/EdenExplorer?style=for-the-badge&color=5F4B87&labelColor=14161A" />
</p>
    </td>
    <td width="40%" align="right">
  <img src="src/assets/icon.ico" alt="EdenExplorer Icon" width="128" height="128">
    </td>
  </tr>
</table>

## ⭐ Support

If you like this FOSS project, consider sponsoring

[![Sponsor](https://img.shields.io/badge/Sponsor-%E2%9D%A4-5F4B87?style=for-the-badge&labelColor=14161A&logo=github)](https://github.com/sponsors/mtucciarone)


## ⚡ Why EdenExplorer?

Windows File Explorer hasn't evolved for modern workflows. 

It's slow, inefficient, and built for a different era.

**EdenExplorer fixes that.**

**Powered by direct NT-level filesystem access for maximum performance.**

* ⚡ **Lightning-fast performance** — minimal overhead
* 🧠 **Efficient by design** — Built in Rust for memory safety and speed
* 🎯 **Minimal, modern UI** — Clean, distraction-free interface that just works
* 🔓 **100% Free & Open Source** — No telemetry, no lock-in, no nonsense
* 🪶 **Lightweight footprint** — Uses a fraction of the resources of Explorer
* 🧰 **Built for daily use** — Your new go-to file manager for everything

## 🧩 Built With Modern Technology

* 🦀 **Rust** — Safe, fast, and reliable systems programming
* 🎨 [**egui**](https://github.com/emilk/egui/) — Immediate mode GUI for ultra-responsive interfaces
* 📦 [**egui-phosphor**](https://github.com/amPerl/egui-phosphor) — [Phosphor](https://github.com/phosphor-icons/homepage) icon set for egui
* ⚙️ **NT-level filesystem access** — Maximum performance, minimal abstraction

## 🆚 Comparison

| Feature                | EdenExplorer | FilePilot | Windows Explorer |
|----------------------|-------------|------------------|------------------|
| Pricing              | ✅ Free      | ❌ Full-Version Paid          | ❌ Your Data          |
| Performance          | ⚡ Fast      | ⚡ Fast          | 🐢 Slow          |
| Open Source          | ✅ Yes       | ❌ No            | ❌ No            |
| NT-level access      | ✅ Yes       | ✅ Yes           | ❌ No            |
| Resource usage       | 🪶 Low       | 🪶 Low        | 🧱 Heavy         |

## 🚀 Getting Started
### Download
Grab the latest release from:
https://github.com/mtucciarone/EdenExplorer/releases

Just download and launch — no installation, no setup.

## ✨ Features

### 🚀 Core Functionality
- **Lightning-fast GUI** that starts at the **root of your computer**, displaying all drives with comprehensive storage types and detailed information
- **Asynchronous directory scanning** for ultra-fast file listing without blocking the UI
- **Intuitive navigation** with **Back / Forward / Up** controls for seamless browsing
- **Smart sidebar** with quick access to common folders (Desktop, Documents, Downloads) and customizable favorites

### 🎯 User Interface & Navigation
- **Tabbed navigation** with independent loading states
- **Interactive breadcrumb navigation** with clickable path segments and inline path editing
- **Responsive design** that adapts to different window sizes and configurations
- **Modern toolbar** with file operations and folder creation tools

### 🎨 Theme & Customization
- **Dark/Light mode switching** with instant toggle via topbar button
- **Advanced theme customization** with comprehensive color palette editor
- **Customizable startup directory** - set your preferred default location
- **Persistent settings** that survive application restarts

### ⚡ Advanced Features
- **Favorites system** with drive-specific storage and drag-and-drop support
- **File operations history** with undo/redo functionality
- **Background folder size calculation** with progress updates and user control
- **Context menu operations** (cut, copy, paste, rename, delete)
- **Drag and drop files/folders** - Move one or more items into folders shown in the item viewer
- **Portable device support** for iPhone, Android, and connected devices
- **Raw/unmounted drive detection** for ISO sticks and Linux partitions

### 🔍 Search & Filtering
- **Real-time file filtering** - typing characters automatically start filtering items in the item viewer
- **Fuzzy matching** for intelligent search results
- **Performance-optimized filtering** with cached indices for instant results

### 🪟 System Integration
- **Persistent settings** using binary cache format surviving application restarts
- **Efficient drive space queries** with intelligent caching
- **Windows API integration** for optimal performance and compatibility
- **Custom executable icon** with proper Windows file association
- **Window management improvements** with proper maximization bounds and minimum size constraints

### ⚡ Performance Optimizations
- **NT-level filesystem access** via direct NT API calls
- **Background scanning** prevents UI freezing during large directory operations
- **Efficient caching** for frequently accessed directories and metadata
- **Streaming directory enumeration** with optimized buffer management
- **Low memory footprint** optimized for Windows 11 environments
- **Performance benchmarking system** with real-time measurement and comparison tools

## 🗺️ Roadmap

### ✅ Implemented Features
- [x] **Tabbed interface** with tab management and navigation
- [x] **Search and filter engine** with real-time file indexing
- [x] **Dark/Light theme switching** with toggle controls
- [x] **Comprehensive navigation** with back/forward/up controls
- [x] **Favorites system** with drag-and-drop support
- [x] **Favorites management** with reset and reorganization capabilities
- [x] **Context menu operations** (cut, copy, paste, rename, delete)
- [x] **Enhanced drive caching** with 30-second cache duration for improved UI performance
- [x] **Optimized icon caching** with metadata-based cache keys and background loading
- [x] **Folder size scanning control** with user setting to enable/disable performance-heavy operations
- [x] **Window size customization** with fullscreen, half-screen, and custom dimension modes
- [x] **Portable device support** for iPhone, Android, and other connected devices
- [x] **Raw/unmounted drive detection** for ISO sticks and Linux partitions
- [x] **Performance benchmarking system** with real-time measurement and comparison tools
- [x] **Drag and drop files/folders** - Move one or more items into folders shown in the item viewer
- [x] **Window management improvements** with proper maximization bounds and minimum size constraints
- [x] **File/Directory filtering** - typing characters automatically start filtering items in the item viewer

### 🚀 Upcoming Features
- [ ] **Image previews using Spacebar** - GPU texture via wgpu / egui_wgpu_backend
  - Decodes image once, uploads to GPU, renders instantly
  - Even very large images (>10k×10k) show instantly
  - Minimal CPU overhead
  - Best for "popup over app" with no lag
- [ ] Drag and drop files into breadcrumb folders
- [ ] Support network devices

## 🐛 Known Bugs
- None currently

## Star History

<a href="https://www.star-history.com/?repos=mtucciarone%2FEdenExplorer&type=date&legend=top-left">
 <picture>
   <source media="(prefers-color-scheme: dark)" srcset="https://api.star-history.com/image?repos=mtucciarone/EdenExplorer&type=date&theme=dark&legend=top-left" />
   <source media="(prefers-color-scheme: light)" srcset="https://api.star-history.com/image?repos=mtucciarone/EdenExplorer&type=date&legend=top-left" />
   <img alt="Star History Chart" src="https://api.star-history.com/image?repos=mtucciarone/EdenExplorer&type=date&legend=top-left" />
 </picture>
</a>

## License
This project is FOSS, released under the MIT License.