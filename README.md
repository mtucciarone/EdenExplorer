# ExplorerEden

**ExplorerEden** is a blazing-fast, minimal Windows 11 file explorer built in Rust with egui.  
It is designed for performance, direct NT-level filesystem scanning, and low memory overhead.  
---

## Features

- GUI that starts at **My PC**, showing all drives with type and basic info.
- Async directory scanning for ultra-fast file listing.
- Navigation: **Back / Forward / Up**.
- Sidebar with common folders (Desktop, Home) and favorites.
- Toolbar with **New Folder** functionality.
- Modular design for swapping in ultra-fast NT API scanning later.

---

## Requirements

- **Windows 11** (or Windows 10+)
- **Rust** (latest stable or nightly)
- **Cargo** (comes with Rust)
- **Visual Studio Build Tools** with C++ Desktop development tools

---

## Installation
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
git clone https://github.com/yourusername/explorereden.git
cd explorereden
```

Build and run in debug mode:
```powershell
cargo run
```

Or build release mode for optimized performance:
```powershell
cargo build --release
.\target\release\explorereden.exe
```

## Contributing
Fork the repository
Make your changes in a new branch
Submit a pull request with a description of your changes

## Future Plans
- Replace `std::fs::read_dir` with NT API (NtQueryDirectoryFile) scanning for maximum speed.
- Add virtualized file list rendering for directories with 100k+ files.
- Implement bookmarks and favorites persistence.
- Optionally, custom icons and metadata without slowing down performance.

## License
This project is FOSS, released under the MIT License.