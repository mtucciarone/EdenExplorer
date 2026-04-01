<p align="center">
  <img src="src/assets/icon.ico" alt="EdenExplorer Icon" width="128" height="128">
</p>

# EdenExplorer — The Ultimate Open Source Windows File Manager 🚀

**EdenExplorer** is a next-generation, **blazing-fast**, fully open-source file explorer built for Windows 11+ using **Rust** and **egui**.
Designed from the ground up for **performance, efficiency, and modern workflows**, EdenExplorer is the **best FOSS alternative** to the default Windows File Explorer.

## ⭐ Support

If you like this FOSS project, consider sponsoring

[![Sponsor](https://img.shields.io/badge/Sponsor-%E2%9D%A4-pink?logo=GitHub&style=for-the-badge)](https://github.com/sponsors/mtucciarone)


## ⚡ Why EdenExplorer?

Windows File Explorer hasn't kept up with power users. It's slow, bloated, and resource-heavy.

**EdenExplorer changes that.**

* ⚡ **Lightning-fast performance** — Direct NT-level filesystem scanning with minimal overhead
* 🧠 **Efficient by design** — Built in Rust for memory safety and speed
* 🎯 **Minimal, modern UI** — Clean, distraction-free interface that just works
* 🔓 **100% Free & Open Source** — No telemetry, no lock-in, no nonsense
* 🪶 **Lightweight footprint** — Uses a fraction of the resources of Explorer
* 🧰 **Built for daily use** — Your new go-to file manager for everything


## 🧩 Built With Modern Technology

* 🦀 **Rust** — Safe, fast, and reliable systems programming
* 🎨 **egui** — Immediate mode GUI for ultra-responsive interfaces
* ⚙️ **NT-level filesystem access** — Maximum performance, minimal abstraction

## ⭐ Try It. Star It. Replace Explorer.

If EdenExplorer improves your workflow, consider giving it a ⭐ on GitHub and contributing to the project.

**Fast. Clean. Open. Powerful.**
That's EdenExplorer.

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