//! Embed the program icon into the Windows .exe (Explorer/Start show it
//! before launch). The icon art is generated procedurally here — same look
//! as the in-app window icon — so there is no art asset to manage.
//!
//! Windows-only; a no-op everywhere else (Linux is the primary target).

fn main() {
    #[cfg(windows)]
    win::embed_icon();
}

#[cfg(windows)]
mod win {
    /// Draw the CRT tile + play triangle at `s`×`s` into RGBA8.
    fn draw(s: usize) -> Vec<u8> {
        let bg = [2u8, 15, 4];
        let green = [0u8, 255, 65];
        let mut px = vec![0u8; s * s * 4];
        let radius = s as f32 * 0.18;
        let inside = |x: f32, y: f32| {
            let mut dx = 0.0f32;
            let mut dy = 0.0f32;
            if x < radius {
                dx = radius - x;
            } else if x > s as f32 - radius {
                dx = x - (s as f32 - radius);
            }
            if y < radius {
                dy = radius - y;
            } else if y > s as f32 - radius {
                dy = y - (s as f32 - radius);
            }
            dx * dx + dy * dy <= radius * radius
        };
        let (ax, ay) = (s as f32 * 0.37, s as f32 * 0.27);
        let (bx, by) = (s as f32 * 0.37, s as f32 * 0.73);
        let (cx, cy) = (s as f32 * 0.73, s as f32 * 0.50);
        let sign = |px: f32, py: f32, x1: f32, y1: f32, x2: f32, y2: f32| {
            (px - x2) * (y1 - y2) - (x1 - x2) * (py - y2)
        };
        for y in 0..s {
            for x in 0..s {
                let i = (y * s + x) * 4;
                let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                if !inside(fx, fy) {
                    px[i + 3] = 0;
                    continue;
                }
                let mut col = bg;
                if s >= 48 && y % 6 < 1 {
                    col = [1, 9, 3];
                }
                let edge = (x as f32)
                    .min(y as f32)
                    .min((s - 1 - x) as f32)
                    .min((s - 1 - y) as f32);
                if edge < (s as f32 * 0.05).max(1.0) {
                    col = green;
                }
                let d1 = sign(fx, fy, ax, ay, bx, by);
                let d2 = sign(fx, fy, bx, by, cx, cy);
                let d3 = sign(fx, fy, cx, cy, ax, ay);
                let neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
                let pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
                if !(neg && pos) {
                    col = green;
                }
                px[i] = col[0];
                px[i + 1] = col[1];
                px[i + 2] = col[2];
                px[i + 3] = 255;
            }
        }
        px
    }

    pub fn embed_icon() {
        let out = std::path::Path::new(&std::env::var("OUT_DIR").unwrap())
            .join("nexus.ico");
        let mut dir = ico::IconDir::new(ico::ResourceType::Icon);
        for &sz in &[256usize, 64, 48, 32, 16] {
            let img = ico::IconImage::from_rgba_data(sz as u32, sz as u32, draw(sz));
            match ico::IconDirEntry::encode(&img) {
                Ok(e) => dir.add_entry(e),
                Err(e) => {
                    println!("cargo:warning=icon encode {sz} failed: {e}");
                    return;
                }
            }
        }
        let f = match std::fs::File::create(&out) {
            Ok(f) => f,
            Err(e) => {
                println!("cargo:warning=icon write failed: {e}");
                return;
            }
        };
        if let Err(e) = dir.write(f) {
            println!("cargo:warning=icon serialize failed: {e}");
            return;
        }
        let mut res = winresource::WindowsResource::new();
        res.set_icon(out.to_str().unwrap());
        if let Err(e) = res.compile() {
            // Missing rc.exe / SDK shouldn't break the build — just no .exe icon.
            println!("cargo:warning=resource embed skipped: {e}");
        }
    }
}
