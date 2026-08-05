extern crate winres;

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap() == "windows" {
        let mut res = winres::WindowsResource::new();
        res.set_icon("icon.ico");
        res.set("FileDescription", "Antigravity Configuration Tool");
        res.set("ProductName", "Antigravity Configurator");
        res.set("LegalCopyright", "Brent t.me/nova_txt");
        // Kept in step with Cargo.toml, which build_rust.py rewrites per release.
        let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
        res.set("FileVersion", &version);
        res.set("ProductVersion", &version);
        res.compile().unwrap();
    }
}
