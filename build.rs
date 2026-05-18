//! Embed the program icon into the Windows .exe so Explorer/Start show it
//! before launch. The icon is `icon.ico` at the repo root — the same file the
//! in-app window icon uses. Swap that file to change the icon.
//!
//! Windows-only; a no-op everywhere else (Linux is the primary target).

fn main() {
    println!("cargo:rerun-if-changed=icon.ico");
    #[cfg(windows)]
    win::embed_icon();
}

#[cfg(windows)]
mod win {
    pub fn embed_icon() {
        if !std::path::Path::new("icon.ico").exists() {
            println!("cargo:warning=icon.ico missing — .exe will have no icon");
            return;
        }
        let mut res = winresource::WindowsResource::new();
        res.set_icon("icon.ico");
        if let Err(e) = res.compile() {
            // Missing rc.exe / SDK shouldn't break the build — just no .exe icon.
            println!("cargo:warning=resource embed skipped: {e}");
        }
    }
}
