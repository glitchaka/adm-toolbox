use std::{
    env,
    path::{Path, PathBuf},
};

pub fn home_dir() -> PathBuf {
    env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("C:\\"))
}

pub fn resolve(cwd: &Path, raw: &str) -> PathBuf {
    if raw == "~" {
        return home_dir();
    }

    if let Some(rest) = raw.strip_prefix("~/").or_else(|| raw.strip_prefix("~\\")) {
        return home_dir().join(rest);
    }

    #[cfg(target_os = "windows")]
    if raw.len() >= 3 && raw.starts_with('/') && raw.as_bytes()[2] == b'/' {
        let drive = raw.chars().nth(1).unwrap_or('c').to_ascii_uppercase();
        let rest = &raw[3..];
        return PathBuf::from(format!("{drive}:\\")).join(rest.replace('/', "\\"));
    }

    let path = PathBuf::from(raw);
    if path.is_absolute() { path } else { cwd.join(path) }
}

pub fn display(path: &Path) -> String {
    let home = home_dir();

    #[cfg(target_os = "windows")]
    {
        let path_text = path.to_string_lossy().replace('\\', "/");
        let home_text = home.to_string_lossy().replace('\\', "/");
        let path_lower = path_text.to_ascii_lowercase();
        let home_lower = home_text.to_ascii_lowercase();

        if path_lower == home_lower {
            return "~".to_owned();
        }

        if path_lower.starts_with(&(home_lower.clone() + "/")) {
            return format!("~/{}", &path_text[home_text.len() + 1..]);
        }

        let bytes = path_text.as_bytes();
        if bytes.len() >= 3 && bytes[1] == b':' && bytes[2] == b'/' {
            let drive = (bytes[0] as char).to_ascii_lowercase();
            let rest = &path_text[3..];
            return if rest.is_empty() {
                format!("/{drive}")
            } else {
                format!("/{drive}/{rest}")
            };
        }

        path_text
    }

    #[cfg(not(target_os = "windows"))]
    {
        if path == home {
            "~".to_owned()
        } else if let Ok(relative) = path.strip_prefix(&home) {
            format!("~/{}", relative.display())
        } else {
            path.to_string_lossy().into_owned()
        }
    }
}
