
extern crate winres;

fn main() {
    // Tell Cargo to rerun build script if icon changes
    println!("cargo:rerun-if-changed=src/icon.ico");

    #[cfg(target_os = "windows")]
    {
        let mut res = winres::WindowsResource::new();

        res.set_icon("src/icon.ico");

        res.set("FileDescription", "EdenExplorer");
        res.set("ProductName", "EdenExplorer");
        res.set("FileVersion", env!("CARGO_PKG_VERSION"));
        res.set("ProductVersion", env!("CARGO_PKG_VERSION"));
        res.set("OriginalFilename", "EdenExplorer.exe");

        res.compile()
            .expect("Failed to compile Windows resources");
    }
}