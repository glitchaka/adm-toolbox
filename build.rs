use std::{
    env,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

const FONT_URL: &str =
    "https://raw.githubusercontent.com/ryanoasis/nerd-fonts/v3.3.0/patched-fonts/JetBrainsMono/Ligatures/Regular/JetBrainsMonoNerdFontMono-Regular.ttf";
const FONT_NAME: &str = "JetBrainsMonoNerdFontMono-Regular.ttf";

fn main() {
    println!("cargo:rerun-if-env-changed=ADM_NERD_FONT_FILE");
    println!("cargo:rerun-if-changed=assets/shell-shock-mascot.svg");

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR no definido"));
    ensure_nerd_font(&out_dir);

    #[cfg(windows)]
    {
        let icon = out_dir.join("shell-shock.ico");
        write_shell_shock_icon(&icon).expect("No se pudo generar el icono de Shell Shock Tool");

        let mut resource = winres::WindowsResource::new();
        resource.set_icon(
            icon.to_str()
                .expect("La ruta del icono de Shell Shock Tool no es UTF-8"),
        );
        resource
            .compile()
            .expect("No se pudo incrustar el icono de Shell Shock Tool");
    }
}

fn ensure_nerd_font(out_dir: &Path) {
    let destination = out_dir.join(FONT_NAME);

    if let Some(source) = env::var_os("ADM_NERD_FONT_FILE") {
        fs::copy(Path::new(&source), &destination)
            .expect("No se pudo copiar ADM_NERD_FONT_FILE");
    } else if !destination.is_file() {
        let status = Command::new("curl")
            .args([
                "-L",
                "--fail",
                "--silent",
                "--show-error",
                FONT_URL,
                "-o",
            ])
            .arg(&destination)
            .status()
            .expect("No se pudo ejecutar curl para obtener la Nerd Font durante la compilación");

        if !status.success() {
            panic!(
                "No se pudo obtener la Nerd Font. Compile con Internet o defina ADM_NERD_FONT_FILE apuntando a JetBrainsMono Nerd Font Mono."
            );
        }
    }

    let metadata = fs::metadata(&destination)
        .expect("La Nerd Font embebida no existe");
    if metadata.len() < 100_000 {
        panic!("La Nerd Font embebida parece incompleta");
    }
}

#[cfg(windows)]
fn write_shell_shock_icon(path: &Path) -> std::io::Result<()> {
    const W: usize = 64;
    const H: usize = 64;
    let mut rgba = vec![[0u8; 4]; W * H];

    rounded_rect(&mut rgba, W, H, 2, 7, 60, 50, 11, [22, 24, 28, 255]);
    rounded_rect(&mut rgba, W, H, 5, 13, 54, 40, 8, [205, 239, 255, 255]);
    rounded_rect(&mut rgba, W, H, 7, 15, 50, 36, 7, [216, 245, 255, 255]);

    circle(&mut rgba, W, H, 46, 10, 2, [246, 198, 27, 255]);
    circle(&mut rgba, W, H, 52, 10, 2, [31, 168, 245, 255]);
    circle(&mut rgba, W, H, 58, 10, 2, [255, 45, 56, 255]);

    line(&mut rgba, W, H, 11, 24, 16, 29, 2, [42, 45, 51, 255]);
    line(&mut rgba, W, H, 16, 29, 11, 34, 2, [42, 45, 51, 255]);

    ellipse(&mut rgba, W, H, 23, 29, 4, 8, [8, 9, 11, 255]);
    ellipse(&mut rgba, W, H, 43, 29, 4, 8, [8, 9, 11, 255]);
    circle(&mut rgba, W, H, 22, 26, 1, [255, 255, 255, 255]);
    circle(&mut rgba, W, H, 42, 26, 1, [255, 255, 255, 255]);

    ellipse(&mut rgba, W, H, 20, 40, 4, 2, [255, 159, 189, 190]);
    ellipse(&mut rgba, W, H, 46, 40, 4, 2, [255, 159, 189, 190]);
    ellipse(&mut rgba, W, H, 33, 41, 5, 2, [9, 9, 11, 255]);

    let mask_stride = ((W + 31) / 32) * 4;
    let xor_size = W * H * 4;
    let mask_size = mask_stride * H;
    let image_size = 40 + xor_size + mask_size;
    let mut out = Vec::with_capacity(22 + image_size);

    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());

    out.push(W as u8);
    out.push(H as u8);
    out.push(0);
    out.push(0);
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&(image_size as u32).to_le_bytes());
    out.extend_from_slice(&22u32.to_le_bytes());

    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(W as i32).to_le_bytes());
    out.extend_from_slice(&((H * 2) as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&((xor_size + mask_size) as u32).to_le_bytes());
    out.extend_from_slice(&0i32.to_le_bytes());
    out.extend_from_slice(&0i32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());

    for y in (0..H).rev() {
        for x in 0..W {
            let [r, g, b, a] = rgba[y * W + x];
            out.extend_from_slice(&[b, g, r, a]);
        }
    }

    for y in (0..H).rev() {
        let mut row = vec![0u8; mask_stride];
        for x in 0..W {
            if rgba[y * W + x][3] == 0 {
                row[x / 8] |= 0x80 >> (x % 8);
            }
        }
        out.extend_from_slice(&row);
    }

    fs::write(path, out)
}

#[cfg(windows)]
fn rounded_rect(
    pixels: &mut [[u8; 4]],
    width: usize,
    height: usize,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    radius: i32,
    color: [u8; 4],
) {
    for py in y..y + h {
        for px in x..x + w {
            let dx = if px < x + radius {
                x + radius - px
            } else if px >= x + w - radius {
                px - (x + w - radius - 1)
            } else {
                0
            };
            let dy = if py < y + radius {
                y + radius - py
            } else if py >= y + h - radius {
                py - (y + h - radius - 1)
            } else {
                0
            };
            if dx * dx + dy * dy <= radius * radius {
                put(pixels, width, height, px, py, color);
            }
        }
    }
}

#[cfg(windows)]
fn circle(
    pixels: &mut [[u8; 4]],
    width: usize,
    height: usize,
    cx: i32,
    cy: i32,
    radius: i32,
    color: [u8; 4],
) {
    ellipse(pixels, width, height, cx, cy, radius, radius, color);
}

#[cfg(windows)]
fn ellipse(
    pixels: &mut [[u8; 4]],
    width: usize,
    height: usize,
    cx: i32,
    cy: i32,
    rx: i32,
    ry: i32,
    color: [u8; 4],
) {
    for y in cy - ry..=cy + ry {
        for x in cx - rx..=cx + rx {
            let dx = (x - cx) as f32 / rx.max(1) as f32;
            let dy = (y - cy) as f32 / ry.max(1) as f32;
            if dx * dx + dy * dy <= 1.0 {
                put(pixels, width, height, x, y, color);
            }
        }
    }
}

#[cfg(windows)]
fn line(
    pixels: &mut [[u8; 4]],
    width: usize,
    height: usize,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    thickness: i32,
    color: [u8; 4],
) {
    let steps = (x1 - x0).abs().max((y1 - y0).abs()).max(1);
    for step in 0..=steps {
        let t = step as f32 / steps as f32;
        let x = (x0 as f32 + (x1 - x0) as f32 * t).round() as i32;
        let y = (y0 as f32 + (y1 - y0) as f32 * t).round() as i32;
        circle(pixels, width, height, x, y, thickness, color);
    }
}

#[cfg(windows)]
fn put(
    pixels: &mut [[u8; 4]],
    width: usize,
    height: usize,
    x: i32,
    y: i32,
    color: [u8; 4],
) {
    if x >= 0 && y >= 0 && (x as usize) < width && (y as usize) < height {
        pixels[y as usize * width + x as usize] = color;
    }
}
