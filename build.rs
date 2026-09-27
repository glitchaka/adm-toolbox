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

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR no definido"));
    let destination = out_dir.join(FONT_NAME);

    if let Some(source) = env::var_os("ADM_NERD_FONT_FILE") {
        fs::copy(Path::new(&source), &destination)
            .expect("No se pudo copiar ADM_NERD_FONT_FILE");
        return;
    }

    if destination.is_file() {
        return;
    }

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

    let metadata = fs::metadata(&destination)
        .expect("La Nerd Font descargada no existe");
    if metadata.len() < 100_000 {
        panic!("La Nerd Font descargada parece incompleta");
    }
}
