use std::fs;
use std::path::PathBuf;

fn main() {
    // ==============================
    // 🪟 Windows EXE metadata + icon
    // ==============================
    #[cfg(target_os = "windows")]
    {
        let mut res = winres::WindowsResource::new();

        res.set_icon("src/assets/icon.ico");

        res.set("FileDescription", "EdenExplorer");
        res.set("ProductName", "EdenExplorer");
        res.set("FileVersion", env!("CARGO_PKG_VERSION"));
        res.set("ProductVersion", env!("CARGO_PKG_VERSION"));
        res.set("OriginalFilename", "EdenExplorer.exe");

        res.compile()
            .expect("Failed to compile Windows resources");
    }
}