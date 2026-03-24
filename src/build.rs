fn main() {
    let mut res = winres::WindowsResource::new();
    res.set_icon("src/assets/icon.ico");
    res.set("FileDescription", "EdenExplorer");
    res.set("ProductName", "EdenExplorer");
    res.set("FileVersion", "1.0.0");
    res.set("ProductVersion", "1.0.0");
    res.compile().unwrap();
}