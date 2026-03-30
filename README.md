<p align="center">
  <img src="src/assets/icon.ico" alt="EdenExplorer Icon" width="128" height="128">
</p>

# EdenExplorer — The Ultimate Open Source Windows File Manager 🚀

**EdenExplorer** is a next-generation, **blazing-fast**, fully open-source file explorer built for Windows 11+ using **Rust** and **egui**.
Designed from the ground up for **performance, efficiency, and modern workflows**, EdenExplorer is the **best FOSS alternative** to the default Windows File Explorer.

---

## ⚡ Why EdenExplorer?

Windows File Explorer hasn't kept up with power users. It's slow, bloated, and resource-heavy.

**EdenExplorer changes that.**

* ⚡ **Lightning-fast performance** — Direct NT-level filesystem scanning with minimal overhead
* 🧠 **Efficient by design** — Built in Rust for memory safety and speed
* 🎯 **Minimal, modern UI** — Clean, distraction-free interface that just works
* 🔓 **100% Free & Open Source** — No telemetry, no lock-in, no nonsense
* 🪶 **Lightweight footprint** — Uses a fraction of the resources of Explorer
* 🧰 **Built for daily use** — Your new go-to file manager for everything

---

## 🚀 A True Windows Explorer Replacement

EdenExplorer isn't just an alternative — it's a **drop-in upgrade**.

Whether you're:

* Navigating large directories
* Managing files at scale
* Working with development environments
* Or just browsing your system daily

EdenExplorer delivers a **consistently fast, smooth experience** without the lag.

---

## 🧩 Built With Modern Technology

* 🦀 **Rust** — Safe, fast, and reliable systems programming
* 🎨 **egui** — Immediate mode GUI for ultra-responsive interfaces
* ⚙️ **NT-level filesystem access** — Maximum performance, minimal abstraction

---

## 💡 Designed for Power Users (Without Feeling Heavy)

EdenExplorer strikes the perfect balance:

* Not overly complex
* Not overly minimal
* Just the **right amount of power and simplicity**

---

## 🌱 The Future of File Management

EdenExplorer is actively evolving to become the **best open-source file manager on Windows**.

If you're tired of slow file operations and unnecessary UI clutter, it's time to switch.

---

## ⭐ Try It. Star It. Replace Explorer.

If EdenExplorer improves your workflow, consider giving it a ⭐ on GitHub and contributing to the project.

**Fast. Clean. Open. Powerful.**
That's EdenExplorer.


## Technical Architecture Comparison

#### **Performance Architecture**
**EdenExplorer:**
- **NT-level filesystem access** via direct `NtQueryDirectoryFile` API calls with `FILE_DIRECTORY_INFORMATION` structures
- **Zero-copy directory enumeration** using 64KB buffers with `IO_STATUS_BLOCK` for minimal allocations
- **Asynchronous directory scanning** via `crossbeam-channel` for thread-safe communication preventing UI freezes
- **Background file operations** with progress callbacks using concurrent message passing
- **Direct Windows API integration** via `windows` crate for optimal performance and compatibility
- **Parallel folder size calculation** with recursive directory traversal and progress emission
- **Efficient drive space queries** using `GetDiskFreeSpaceExW` with intelligent caching
- **Icon caching system** with background loading using `SHGetFileInfoW` and `SHGetImageList`

**Windows File Explorer:**
- **Win32 API layer** with multiple abstraction overheads
- **Polling-based change detection** causing unnecessary I/O
- **Shell namespace extensions** adding complexity and latency
- **Synchronous operations** blocking the UI thread
- **Multiple memory copies** through various API boundaries

#### **💾 Memory Efficiency**
**EdenExplorer:**
- **Rust's ownership model** ensures memory safety without garbage collection pauses
- **Streaming directory enumeration** using 64KB buffers vs. File Explorer's multiple allocations
- **Lazy loading** of file metadata only when needed with on-demand computation
- **Binary cache format** using `bincode` for compact, fast serialization of settings and favorites
- **Efficient string handling** with UTF-16 to UTF-8 conversion only when necessary
- **Icon caching system** using `Arc<Mutex<HashMap>>` for thread-safe shared texture storage
- **Background icon loading** via `crossbeam-channel` to prevent UI memory spikes

**Windows File Explorer:**
- **COM-based architecture** with reference counting overhead
- **Multiple cache layers** causing memory bloat
- **Preloading of thumbnails** and metadata even when not displayed
- **Shell extensions** loading into process space increasing memory footprint

#### **⚡ Data Management**
**EdenExplorer:**
- **Persistent settings** using binary cache format surviving application restarts
- **Favorites system** with drive-specific storage for quick access
- **Background folder size calculation** with progress updates
- **Efficient drive space queries** with caching
- **Tab-based navigation** with independent loading states

**Windows File Explorer:**
- **Windows Search Index** separate process with IPC overhead
- **Delayed indexing** causing search result staleness
- **No persistence** of navigation state across sessions
- **Single-threaded operations**

#### **🎯 UI Responsiveness**
**EdenExplorer:**
- **egui immediate mode GUI** with single-pass rendering
- **Asynchronous directory scanning** preventing UI freezes
- **Background file operations** with progress callbacks
- **Tab-based navigation** with independent loading states
- **Minimal redraw cycles** using dirty region tracking

**Windows File Explorer:**
- **Retained mode GUI** with complex message handling
- **Synchronous file operations** blocking UI thread
- **Shell namespace navigation** causing recursive loading delays
- **Single-window interface** forcing context switches

#### **🔧 Low-level Optimizations**
**EdenExplorer:**
- **Direct NTFS access** reading file records without path resolution
- **Batch I/O operations** minimizing system call overhead
- **Custom time formatting** avoiding expensive locale operations
- **Drive space queries** via `GetDiskFreeSpaceExW` with caching
- **Folder size calculation** using NT API vs. recursive enumeration

**Windows File Explorer:**
- **Multiple API layers** (Shell → Win32 → NT) adding latency
- **Individual file queries** instead of batch operations
- **Complex shell extensions** adding processing overhead
- **Legacy compatibility** code paths for older Windows versions

#### **📊 Performance Benchmarking**

EdenExplorer includes a built-in benchmarking system to measure actual performance metrics:

**Available Benchmarks:**
- **Directory scanning** - Time to enumerate files and folders
- **Folder size calculation** - Recursive size computation performance  
- **Application startup** - Cold start timing
- **Memory usage** - Resource consumption analysis

**Running Benchmarks:**
```rust
use eden_explorer::core::benchmark::run_comprehensive_benchmark;

// Run benchmarks on a test directory
let results = run_comprehensive_benchmark(PathBuf::from("C:\\Windows\\System32"));
println!("{}", results);
```

**Expected Performance Targets:**
Based on architectural advantages, EdenExplorer targets:
- **Directory listing**: 10-15x faster than Windows Explorer
- **Folder size calculation**: 15-25x faster than Windows Explorer  
- **Memory usage**: 3-5x lower than Windows Explorer
- **Startup time**: 2-3x faster than Windows Explorer

*Note: Actual performance varies by system specifications and directory complexity. Run the benchmark suite on your system for accurate measurements.*

#### **🛡️ Reliability & Safety**
**EdenExplorer:**
- **Rust's memory safety** eliminates entire classes of bugs
- **Error propagation** through `Result` types preventing silent failures
- **Resource management** via RAII preventing handle leaks
- **Thread safety** guaranteed at compile time
- **No shell extensions** reducing crash surface area

**Windows File Explorer:**
- **C++ codebase** with manual memory management risks
- **Third-party shell extensions** causing instability
- **Complex error handling** with silent failures
- **Legacy compatibility** code with security implications

### 🎯 Bottom Line
EdenExplorer represents a **fundamentally different approach** to file management, leveraging modern systems programming principles and direct OS integration to deliver performance that simply isn't possible with Windows File Explorer's legacy architecture.

## ✨ Features

### 🚀 Core Functionality
- **Lightning-fast GUI** that starts at the **root of your computer**, displaying all drives with comprehensive storage types and detailed information
- **Asynchronous directory scanning** for ultra-fast file listing without blocking the UI
- **Intuitive navigation** with **Back / Forward / Up** controls for seamless browsing
- **Smart sidebar** with quick access to common folders (Desktop, Documents, Downloads) and customizable favorites

### User Interface Features
- **Tabbed navigation** with independent loading states and tab management
- **Theme customization** with dark/light mode switching and custom theme editor
- **Responsive design** that adapts to different window sizes and configurations
- **Modern toolbar** with file operations and folder creation tools

### Advanced Features
- **Favorites system** with drive-specific storage and drag-and-drop support
- **File operations history** with undo/redo functionality
- **Background folder size calculation** with progress updates
- **Context menu operations** (cut, copy, paste, rename, delete)
- **Search and filter capabilities** with real-time file indexing
- **Portable device support** for iPhone, Android, and connected devices
- **Raw/unmounted drive detection** for ISO sticks and Linux partitions

### System Integration
- **Persistent settings** using binary cache format surviving application restarts
- **Efficient drive space queries** with intelligent caching
- **Windows API integration** for optimal performance and compatibility
- **Custom executable icon** with proper Windows file association

### Performance Optimizations
- **NT-level filesystem access** via direct NT API calls
- **Background scanning** prevents UI freezing during large directory operations
- **Efficient caching** for frequently accessed directories and metadata
- **Streaming directory enumeration** with optimized buffer management
- **Low memory footprint** optimized for Windows 11 environments

---

## Installation

- **Windows 11** (or Windows 10+)
- **Rust** (latest stable or nightly)
- **Cargo** (comes with Rust)
- **Visual Studio Build Tools** with C++ Desktop development tools

---

### Installation Requirements
1. Install Rust:
```powershell
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```
Or download rustup-init.exe and run it.

### Install Visual Studio Build Tools:
Select Desktop development with C++ during install.
Restart your terminal to ensure cargo is in your PATH.

### Build & Run
Clone the repository:
```powershell
git clone https://github.com/yourusername/EdenExplorer.git
cd EdenExplorer
```

Build and run in debug mode:
```powershell
cargo run
```

Or build release mode for optimized performance:
```powershell
cargo build --release
.\target\release\EdenExplorer.exe
```

## Contributing
Fork the repository
Make your changes in a new branch
Submit a pull request with a description of your changes

## 🗺️ Roadmap

### ✅ Implemented Features
- [x] **Tabbed interface** with tab management and navigation
- [x] **Search and filter engine** with real-time file indexing
- [x] **File operations history** with undo/redo functionality
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
- [ ] **Tab navigation improvements** - multiple tabs should reduce tab size, with left/right arrows for horizontal scrolling when >6 tabs
- [ ] Self-signed executable

## 🐛 Known Bugs
- Fix network detection in sidebar
- Fix raw/unmounted drive detection for ISO sticks and Linux partitions
- selection dragging box doesn't select anything
- navigating files with arrow keys when there's no selection doesn't automatically scroll the table to the index of the navigation
- While filtering is active, doing shift+home, deleting all the content, then hitting Escape places the input cursor on next filter at -1 position
- Opening an external drive, like an IPhone, shows "this folder is empty". trying to navigate "up" breaks the interface.

## License
This project is FOSS, released under the MIT License.