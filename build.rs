extern crate winres;

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap() == "windows" {
        let mut res = winres::WindowsResource::new();
        res.set_icon("icon.ico");
        res.set("FileDescription", "Antigravity Configuration Tool");
        res.set("ProductName", "Antigravity Configurator");
        res.set("LegalCopyright", "Brent t.me/nova_txt");
        res.set("FileVersion", "2.3.1.2");
        res.set("ProductVersion", "2.3.1.2");
        res.compile().unwrap();
    }
}
