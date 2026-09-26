use std::{
    env,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

use super::CommandOutput;
use crate::shell::resolve_path;

pub fn run(
    name: &str,
    args: &[String],
    input: Option<&[u8]>,
    cwd: &Path,
) -> Result<CommandOutput> {
    match name {
        "pwd" => Ok(CommandOutput::ok(format!("{}\n", display_unix_path(cwd)))),
        "echo" => Ok(CommandOutput::ok(format!("{}\n", args.join(" ")))),
        "ls" => ls(args, cwd),
        "cat" => cat(args, input, cwd),
        "head" => head_tail(args, input, cwd, true),
        "tail" => head_tail(args, input, cwd, false),
        "grep" => grep(args, input, cwd),
        "wc" => wc(args, input, cwd),
        "sort" => sort(args, input, cwd),
        "uniq" => uniq(args, input, cwd),
        "cut" => cut(args, input, cwd),
        "tee" => tee(args, input, cwd),
        "find" => find(args, cwd),
        "touch" => touch(args, cwd),
        "mkdir" => mkdir(args, cwd),
        "rm" => rm(args, cwd),
        "cp" => cp(args, cwd),
        "mv" => mv(args, cwd),
        "which" => which(args),
        _ => Ok(CommandOutput::error("comando Unix no implementado", 127)),
    }
}

fn ls(args: &[String], cwd: &Path) -> Result<CommandOutput> {
    let show_hidden = args.iter().any(|arg| arg.starts_with('-') && arg.contains('a'));
    let long = args.iter().any(|arg| arg.starts_with('-') && arg.contains('l'));
    let target = args
        .iter()
        .find(|arg| !arg.starts_with('-'))
        .map(|arg| resolve_path(cwd, arg))
        .unwrap_or_else(|| cwd.to_path_buf());

    let mut entries: Vec<_> = fs::read_dir(&target)
        .with_context(|| format!("ls: no se pudo leer {}", target.display()))?
        .filter_map(Result::ok)
        .filter(|entry| show_hidden || !entry.file_name().to_string_lossy().starts_with('.'))
        .collect();

    entries.sort_by_key(|entry| entry.file_name().to_string_lossy().to_lowercase());

    let mut out = String::new();
    for entry in entries {
        let metadata = entry.metadata()?;
        let name = entry.file_name().to_string_lossy().into_owned();

        if long {
            let kind = if metadata.is_dir() { 'd' } else { '-' };
            let readonly = if metadata.permissions().readonly() { 'r' } else { 'w' };
            out.push_str(&format!(
                "{kind}{readonly} {:>12} {name}{}\n",
                metadata.len(),
                if metadata.is_dir() { "/" } else { "" }
            ));
        } else {
            out.push_str(&name);
            if metadata.is_dir() {
                out.push('/');
            }
            out.push('\n');
        }
    }

    Ok(CommandOutput::ok(out))
}

fn cat(args: &[String], input: Option<&[u8]>, cwd: &Path) -> Result<CommandOutput> {
    if args.is_empty() {
        return Ok(CommandOutput::ok(input_text(input)));
    }

    let mut out = String::new();
    for arg in args {
        let path = resolve_path(cwd, arg);
        out.push_str(
            &fs::read_to_string(&path)
                .with_context(|| format!("cat: no se pudo leer {}", path.display()))?,
        );
    }
    Ok(CommandOutput::ok(out))
}

fn head_tail(
    args: &[String],
    input: Option<&[u8]>,
    cwd: &Path,
    head: bool,
) -> Result<CommandOutput> {
    let mut count = 10_usize;
    let mut file = None;
    let mut index = 0;

    while index < args.len() {
        if args[index] == "-n" {
            index += 1;
            if let Some(value) = args.get(index) {
                count = value.parse().unwrap_or(10);
            }
        } else if !args[index].starts_with('-') {
            file = Some(args[index].clone());
        }
        index += 1;
    }

    let text = source_text(file.as_deref(), input, cwd)?;
    let lines: Vec<&str> = text.lines().collect();
    let selected: Vec<&str> = if head {
        lines.into_iter().take(count).collect()
    } else {
        let start = lines.len().saturating_sub(count);
        lines.into_iter().skip(start).collect()
    };

    let mut out = selected.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    Ok(CommandOutput::ok(out))
}

fn grep(args: &[String], input: Option<&[u8]>, cwd: &Path) -> Result<CommandOutput> {
    let ignore_case = args.iter().any(|arg| arg == "-i" || arg == "-in" || arg == "-ni");
    let line_numbers = args.iter().any(|arg| arg == "-n" || arg == "-in" || arg == "-ni");
    let positional: Vec<&String> = args.iter().filter(|arg| !arg.starts_with('-')).collect();

    let Some(pattern) = positional.first() else {
        return Ok(CommandOutput::error("grep: falta patrón", 2));
    };

    let file = positional.get(1).map(|s| s.as_str());
    let text = source_text(file, input, cwd)?;
    let needle = if ignore_case {
        pattern.to_lowercase()
    } else {
        (*pattern).clone()
    };

    let mut out = String::new();
    for (index, line) in text.lines().enumerate() {
        let haystack = if ignore_case {
            line.to_lowercase()
        } else {
            line.to_owned()
        };
        if haystack.contains(&needle) {
            if line_numbers {
                out.push_str(&format!("{}:", index + 1));
            }
            out.push_str(line);
            out.push('\n');
        }
    }

    Ok(CommandOutput {
        status: if out.is_empty() { 1 } else { 0 },
        stdout: out,
        stderr: String::new(),
    })
}

fn wc(args: &[String], input: Option<&[u8]>, cwd: &Path) -> Result<CommandOutput> {
    let text = source_text(args.first().map(String::as_str), input, cwd)?;
    Ok(CommandOutput::ok(format!(
        "{} {} {}\n",
        text.lines().count(),
        text.split_whitespace().count(),
        text.as_bytes().len()
    )))
}

fn sort(args: &[String], input: Option<&[u8]>, cwd: &Path) -> Result<CommandOutput> {
    let reverse = args.iter().any(|arg| arg == "-r");
    let file = args.iter().find(|arg| !arg.starts_with('-')).map(String::as_str);
    let text = source_text(file, input, cwd)?;
    let mut lines: Vec<&str> = text.lines().collect();
    lines.sort_unstable();
    if reverse {
        lines.reverse();
    }
    let mut out = lines.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    Ok(CommandOutput::ok(out))
}

fn uniq(args: &[String], input: Option<&[u8]>, cwd: &Path) -> Result<CommandOutput> {
    let text = source_text(args.first().map(String::as_str), input, cwd)?;
    let mut last: Option<&str> = None;
    let mut out = String::new();

    for line in text.lines() {
        if last != Some(line) {
            out.push_str(line);
            out.push('\n');
            last = Some(line);
        }
    }

    Ok(CommandOutput::ok(out))
}

fn cut(args: &[String], input: Option<&[u8]>, cwd: &Path) -> Result<CommandOutput> {
    let mut delimiter = '\t';
    let mut field = 1_usize;
    let mut file: Option<&str> = None;
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "-d" => {
                index += 1;
                if let Some(value) = args.get(index) {
                    delimiter = value.chars().next().unwrap_or('\t');
                }
            }
            "-f" => {
                index += 1;
                if let Some(value) = args.get(index) {
                    field = value.parse::<usize>().unwrap_or(1).max(1);
                }
            }
            other if !other.starts_with('-') => file = Some(other),
            _ => {}
        }
        index += 1;
    }

    let text = source_text(file, input, cwd)?;
    let mut out = String::new();
    for line in text.lines() {
        if let Some(value) = line.split(delimiter).nth(field - 1) {
            out.push_str(value);
        }
        out.push('\n');
    }
    Ok(CommandOutput::ok(out))
}

fn tee(args: &[String], input: Option<&[u8]>, cwd: &Path) -> Result<CommandOutput> {
    let append = args.iter().any(|arg| arg == "-a");
    let Some(file) = args.iter().find(|arg| !arg.starts_with('-')) else {
        return Ok(CommandOutput::error("tee: falta archivo", 2));
    };

    let text = input_text(input);
    let path = resolve_path(cwd, file);
    let mut options = OpenOptions::new();
    options.create(true).write(true);
    if append {
        options.append(true);
    } else {
        options.truncate(true);
    }
    options.open(path)?.write_all(text.as_bytes())?;
    Ok(CommandOutput::ok(text))
}

fn find(args: &[String], cwd: &Path) -> Result<CommandOutput> {
    let root = args
        .first()
        .filter(|arg| !arg.starts_with('-'))
        .map(|arg| resolve_path(cwd, arg))
        .unwrap_or_else(|| cwd.to_path_buf());

    let mut pattern: Option<&str> = None;
    let mut index = 0;
    while index < args.len() {
        if args[index] == "-name" {
            pattern = args.get(index + 1).map(String::as_str);
            break;
        }
        index += 1;
    }

    let mut out = String::new();
    walk_find(&root, pattern, &mut out)?;
    Ok(CommandOutput::ok(out))
}

fn walk_find(path: &Path, pattern: Option<&str>, out: &mut String) -> Result<()> {
    let metadata = match fs::metadata(path) {
        Ok(value) => value,
        Err(_) => return Ok(()),
    };

    if matches_pattern(path, pattern) {
        out.push_str(&format!("{}\n", display_unix_path(path)));
    }

    if metadata.is_dir() {
        for entry in fs::read_dir(path)? {
            let entry = match entry {
                Ok(value) => value,
                Err(_) => continue,
            };
            walk_find(&entry.path(), pattern, out)?;
        }
    }

    Ok(())
}

fn matches_pattern(path: &Path, pattern: Option<&str>) -> bool {
    let Some(pattern) = pattern else {
        return true;
    };

    let name = path.file_name().and_then(|v| v.to_str()).unwrap_or_default();
    wildcard_match(pattern, name)
}

fn wildcard_match(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }

    if !pattern.contains('*') {
        return pattern == value;
    }

    let parts: Vec<&str> = pattern.split('*').collect();
    let mut cursor = 0_usize;

    for (index, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }

        let Some(found) = value[cursor..].find(part) else {
            return false;
        };

        if index == 0 && !pattern.starts_with('*') && found != 0 {
            return false;
        }

        cursor += found + part.len();
    }

    pattern.ends_with('*') || parts.last().is_none_or(|last| value.ends_with(last))
}

fn touch(args: &[String], cwd: &Path) -> Result<CommandOutput> {
    for arg in args {
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(resolve_path(cwd, arg))?;
    }
    Ok(CommandOutput::ok(""))
}

fn mkdir(args: &[String], cwd: &Path) -> Result<CommandOutput> {
    let parents = args.iter().any(|arg| arg == "-p");
    for arg in args.iter().filter(|arg| !arg.starts_with('-')) {
        let path = resolve_path(cwd, arg);
        if parents {
            fs::create_dir_all(path)?;
        } else {
            fs::create_dir(path)?;
        }
    }
    Ok(CommandOutput::ok(""))
}

fn rm(args: &[String], cwd: &Path) -> Result<CommandOutput> {
    let recursive = args
        .iter()
        .any(|arg| matches!(arg.as_str(), "-r" | "-rf" | "-fr"));
    let force = args.iter().any(|arg| arg.starts_with('-') && arg.contains('f'));

    for arg in args.iter().filter(|arg| !arg.starts_with('-')) {
        let path = resolve_path(cwd, arg);
        if !path.exists() {
            if force {
                continue;
            }
            return Ok(CommandOutput::error(
                format!("rm: no existe {}", path.display()),
                1,
            ));
        }

        if path.is_dir() {
            if recursive {
                fs::remove_dir_all(path)?;
            } else {
                return Ok(CommandOutput::error(
                    format!("rm: {} es un directorio; usa -r", path.display()),
                    1,
                ));
            }
        } else {
            fs::remove_file(path)?;
        }
    }

    Ok(CommandOutput::ok(""))
}

fn cp(args: &[String], cwd: &Path) -> Result<CommandOutput> {
    if args.len() != 2 {
        return Ok(CommandOutput::error("cp: uso: cp SOURCE TARGET", 2));
    }

    fs::copy(resolve_path(cwd, &args[0]), resolve_path(cwd, &args[1]))?;
    Ok(CommandOutput::ok(""))
}

fn mv(args: &[String], cwd: &Path) -> Result<CommandOutput> {
    if args.len() != 2 {
        return Ok(CommandOutput::error("mv: uso: mv SOURCE TARGET", 2));
    }

    fs::rename(resolve_path(cwd, &args[0]), resolve_path(cwd, &args[1]))?;
    Ok(CommandOutput::ok(""))
}

fn which(args: &[String]) -> Result<CommandOutput> {
    let Some(name) = args.first() else {
        return Ok(CommandOutput::error("which: falta comando", 2));
    };

    if super::is_internal(name)
        || matches!(
            name.as_str(),
            "cd" | "clear" | "exit" | "vim" | "edit" | "alias" | "export" | "history"
        )
    {
        return Ok(CommandOutput::ok(format!(
            "{name}: comando interno de ADM Toolbox\n"
        )));
    }

    if let Some(path) = find_in_path(name) {
        return Ok(CommandOutput::ok(format!("{}\n", path.display())));
    }

    Ok(CommandOutput::error(
        format!("which: no se encontró {name}"),
        1,
    ))
}

fn find_in_path(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    let extensions: Vec<String> = env::var("PATHEXT")
        .unwrap_or_else(|_| ".EXE;.CMD;.BAT;.COM".to_owned())
        .split(';')
        .map(str::to_owned)
        .collect();

    for dir in env::split_paths(&path) {
        let direct = dir.join(name);
        if direct.is_file() {
            return Some(direct);
        }

        if Path::new(name).extension().is_none() {
            for ext in &extensions {
                let candidate = dir.join(format!("{name}{ext}"));
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }

    None
}

fn source_text(file: Option<&str>, input: Option<&[u8]>, cwd: &Path) -> Result<String> {
    if let Some(file) = file {
        Ok(fs::read_to_string(resolve_path(cwd, file))?)
    } else {
        Ok(input_text(input))
    }
}

fn input_text(input: Option<&[u8]>) -> String {
    input
        .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
        .unwrap_or_default()
}

fn display_unix_path(path: &Path) -> String {
    let text = path.to_string_lossy();

    #[cfg(target_os = "windows")]
    {
        let bytes = text.as_bytes();
        if bytes.len() >= 3 && bytes[1] == b':' && (bytes[2] == b'\\' || bytes[2] == b'/') {
            let drive = (bytes[0] as char).to_ascii_lowercase();
            let rest = text[3..].replace('\\', "/");
            return if rest.is_empty() {
                format!("/{drive}")
            } else {
                format!("/{drive}/{rest}")
            };
        }
    }

    text.replace('\\', "/")
}
