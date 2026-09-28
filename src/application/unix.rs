use std::{
    env,
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use chrono::Local;
use crossterm::event::{Event, KeyCode};
use flate2::{
    Compression,
    read::GzDecoder,
    write::GzEncoder,
};
use tar::{Archive as TarArchive, Builder as TarBuilder};
use zip::{
    ZipArchive,
    ZipWriter,
    write::FileOptions,
};
use crate::adapters::terminal::io as terminal_io;
use sha2::{Digest, Sha256};
use similar::{ChangeTag, TextDiff};

use anyhow::{Context, Result};

use crate::core::CommandOutput;
use crate::support::path::resolve as resolve_path;

pub struct UnixService;

impl UnixService {
    pub fn execute(
        &self,
        name: &str,
        args: &[String],
        input: Option<&[u8]>,
        cwd: &Path,
    ) -> Result<CommandOutput> {
    match name {
        "pwd" => Ok(CommandOutput::ok(format!("{}\n", display_unix_path(cwd)))),
        "echo" => Ok(CommandOutput::ok(format!("{}\n", args.join(" ")))),
        "env" => env_cmd(),
        "clear" => Ok(CommandOutput::ok("\x1b[2J\x1b[H")),
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
        "less" | "more" => less(args, input, cwd),
        "sed" => sed(args, input, cwd),
        "awk" => awk(args, input, cwd),
        "diff" => diff(args, cwd),
        "sha256sum" => sha256sum(args, input, cwd),
        "base64" => base64_cmd(args, input, cwd),
        "find" => find(args, cwd),
        "printf" => printf(args),
        "basename" => basename(args),
        "dirname" => dirname(args),
        "realpath" => realpath(args, cwd),
        "date" => date(args),
        "sleep" => sleep_cmd(args),
        "true" => Ok(CommandOutput::ok("")),
        "false" => Ok(CommandOutput {
            stdout: String::new(),
            stderr: String::new(),
            status: 1,
        }),
        "touch" => touch(args, cwd),
        "mkdir" => mkdir(args, cwd),
        "rm" => rm(args, cwd),
        "cp" => cp(args, cwd),
        "mv" => mv(args, cwd),
        "tar" => tar_cmd(args, cwd),
        "gzip" | "gunzip" => gzip_cmd(name, args, cwd),
        "zip" => zip_cmd(args, cwd),
        "unzip" => unzip_cmd(args, cwd),
        "which" | "type" => which(args),
        _ => Ok(CommandOutput::error("comando Unix no implementado", 127)),
    }
}

}

fn env_cmd() -> Result<CommandOutput> {
    let mut vars: Vec<_> = env::vars().collect();
    vars.sort_by(|a, b| a.0.cmp(&b.0));

    let mut out = String::new();
    for (name, value) in vars {
        out.push_str(&name);
        out.push('=');
        out.push_str(&value);
        out.push('\n');
    }

    Ok(CommandOutput::ok(out))
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

fn less(args: &[String], input: Option<&[u8]>, cwd: &Path) -> Result<CommandOutput> {
    let file = args.iter().find(|arg| !arg.starts_with('-')).map(String::as_str);
    let text = source_text(file, input, cwd)?;
    let lines: Vec<&str> = text.lines().collect();

    let _guard = crate::adapters::terminal::guard::AlternateScreenGuard::enter()?;
    let mut offset = 0_usize;

    loop {
        let (width, height) = terminal_io::size()?;
        let body = height.saturating_sub(1) as usize;

        let mut screen = String::from("\x1b[H\x1b[2J");

        for line in lines.iter().skip(offset).take(body) {
            let visible: String = line.chars().take(width as usize).collect();
            screen.push_str(&format!("{visible}\n"));
        }

        let percent = if lines.is_empty() {
            100
        } else {
            (((offset + body).min(lines.len()) as f64 / lines.len() as f64) * 100.0) as usize
        };

        screen.push_str(&format!("-- More -- {}%  [j/k PgUp/PgDn g/G q]", percent));
        terminal_io::write(screen.as_bytes())?;

        if let Event::Key(key) = terminal_io::read()? {
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => break,
                KeyCode::Char('j') | KeyCode::Down => {
                    offset = (offset + 1).min(lines.len().saturating_sub(1));
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    offset = offset.saturating_sub(1);
                }
                KeyCode::PageDown | KeyCode::Char(' ') => {
                    offset = (offset + body).min(lines.len().saturating_sub(1));
                }
                KeyCode::PageUp => {
                    offset = offset.saturating_sub(body);
                }
                KeyCode::Char('g') | KeyCode::Home => offset = 0,
                KeyCode::Char('G') | KeyCode::End => {
                    offset = lines.len().saturating_sub(body);
                }
                _ => {}
            }
        }
    }

    Ok(CommandOutput::ok(""))
}

fn sed(args: &[String], input: Option<&[u8]>, cwd: &Path) -> Result<CommandOutput> {
    let positional: Vec<&String> = args.iter().filter(|arg| !arg.starts_with('-')).collect();
    let Some(expression) = positional.first() else {
        return Ok(CommandOutput::error("sed: falta expresión", 2));
    };

    let file = positional.get(1).map(|value| value.as_str());
    let text = source_text(file, input, cwd)?;

    if !expression.starts_with('s') || expression.len() < 3 {
        return Ok(CommandOutput::error(
            "sed: esta versión soporta s/antiguo/nuevo/[g]",
            2,
        ));
    }

    let delimiter = expression.chars().nth(1).unwrap_or('/');
    let body = &expression[2..];
    let parts: Vec<&str> = body.split(delimiter).collect();

    if parts.len() < 2 {
        return Ok(CommandOutput::error(
            "sed: expresión de sustitución inválida",
            2,
        ));
    }

    let pattern = parts[0];
    let replacement = parts[1];
    let flags = parts.get(2).copied().unwrap_or("");
    let global = flags.contains('g');

    let mut out = String::new();
    for line in text.lines() {
        let replaced = if global {
            line.replace(pattern, replacement)
        } else {
            line.replacen(pattern, replacement, 1)
        };
        out.push_str(&replaced);
        out.push('\n');
    }

    Ok(CommandOutput::ok(out))
}

fn awk(args: &[String], input: Option<&[u8]>, cwd: &Path) -> Result<CommandOutput> {
    let mut delimiter: Option<char> = None;
    let mut program: Option<&str> = None;
    let mut file: Option<&str> = None;
    let mut index = 0;

    while index < args.len() {
        if args[index] == "-F" {
            index += 1;
            delimiter = args.get(index).and_then(|value| value.chars().next());
        } else if program.is_none() {
            program = Some(args[index].as_str());
        } else {
            file = Some(args[index].as_str());
        }
        index += 1;
    }

    let Some(program) = program else {
        return Ok(CommandOutput::error("awk: falta programa", 2));
    };

    let text = source_text(file, input, cwd)?;
    let fields = parse_awk_print_fields(program)?;

    let mut out = String::new();

    for (line_no, line) in text.lines().enumerate() {
        let columns: Vec<&str> = if let Some(delimiter) = delimiter {
            line.split(delimiter).collect()
        } else {
            line.split_whitespace().collect()
        };

        let mut values = Vec::new();

        for field in &fields {
            match field.as_str() {
                "$0" => values.push(line.to_owned()),
                "NR" => values.push((line_no + 1).to_string()),
                value if value.starts_with('$') => {
                    let field_index = value[1..].parse::<usize>().unwrap_or(0);
                    values.push(
                        field_index
                            .checked_sub(1)
                            .and_then(|column_index| columns.get(column_index))
                            .copied()
                            .unwrap_or("")
                            .to_owned(),
                    );
                }
                literal => values.push(literal.trim_matches('"').to_owned()),
            }
        }

        out.push_str(&values.join(" "));
        out.push('\n');
    }

    Ok(CommandOutput::ok(out))
}

fn parse_awk_print_fields(program: &str) -> Result<Vec<String>> {
    let trimmed = program.trim();
    let body = trimmed
        .strip_prefix('{')
        .and_then(|value| value.strip_suffix('}'))
        .map(str::trim)
        .unwrap_or(trimmed);

    let Some(rest) = body.strip_prefix("print") else {
        anyhow::bail!("awk: esta versión soporta expresiones {{print ...}}");
    };

    let fields: Vec<String> = rest
        .trim()
        .split(',')
        .flat_map(|chunk| chunk.split_whitespace())
        .map(str::to_owned)
        .collect();

    if fields.is_empty() {
        anyhow::bail!("awk: print sin campos");
    }

    Ok(fields)
}

fn diff(args: &[String], cwd: &Path) -> Result<CommandOutput> {
    if args.len() != 2 {
        return Ok(CommandOutput::error(
            "diff: uso: diff ARCHIVO1 ARCHIVO2",
            2,
        ));
    }

    let left_path = resolve_path(cwd, &args[0]);
    let right_path = resolve_path(cwd, &args[1]);
    let left = fs::read_to_string(&left_path)?;
    let right = fs::read_to_string(&right_path)?;

    if left == right {
        return Ok(CommandOutput::ok(""));
    }

    let diff = TextDiff::from_lines(&left, &right);
    let mut out = format!(
        "--- {}\n+++ {}\n",
        display_unix_path(&left_path),
        display_unix_path(&right_path)
    );

    for change in diff.iter_all_changes() {
        let sign = match change.tag() {
            ChangeTag::Delete => "-",
            ChangeTag::Insert => "+",
            ChangeTag::Equal => " ",
        };

        out.push_str(sign);
        out.push_str(change.value());

        if !change.value().ends_with('\n') {
            out.push('\n');
        }
    }

    Ok(CommandOutput {
        stdout: out,
        stderr: String::new(),
        status: 1,
    })
}

fn sha256sum(args: &[String], input: Option<&[u8]>, cwd: &Path) -> Result<CommandOutput> {
    let data = if let Some(file) = args.first() {
        fs::read(resolve_path(cwd, file))?
    } else {
        input.unwrap_or_default().to_vec()
    };

    let mut hasher = Sha256::new();
    hasher.update(&data);
    let digest = hasher.finalize();

    let label = args.first().map(String::as_str).unwrap_or("-");
    Ok(CommandOutput::ok(format!("{:x}  {label}\n", digest)))
}

fn base64_cmd(args: &[String], input: Option<&[u8]>, cwd: &Path) -> Result<CommandOutput> {
    let decode = args.iter().any(|arg| arg == "-d" || arg == "--decode");
    let file = args
        .iter()
        .find(|arg| !arg.starts_with('-'))
        .map(String::as_str);

    let data = if let Some(file) = file {
        fs::read(resolve_path(cwd, file))?
    } else {
        input.unwrap_or_default().to_vec()
    };

    if decode {
        let text = String::from_utf8_lossy(&data);
        let decoded = BASE64.decode(text.trim().as_bytes())?;
        Ok(CommandOutput::ok(
            String::from_utf8_lossy(&decoded).into_owned(),
        ))
    } else {
        Ok(CommandOutput::ok(format!("{}\n", BASE64.encode(data))))
    }
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

fn printf(args: &[String]) -> Result<CommandOutput> {
    let Some(format) = args.first() else {
        return Ok(CommandOutput::ok(""));
    };

    let mut values = args.iter().skip(1);
    let mut out = String::new();
    let mut chars = format.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
            continue;
        }

        if ch == '%' {
            match chars.peek().copied() {
                Some('%') => {
                    chars.next();
                    out.push('%');
                }
                Some('s') => {
                    chars.next();
                    out.push_str(values.next().map(String::as_str).unwrap_or(""));
                }
                Some('d') => {
                    chars.next();
                    let value = values
                        .next()
                        .and_then(|value| value.parse::<i64>().ok())
                        .unwrap_or(0);
                    out.push_str(&value.to_string());
                }
                _ => out.push('%'),
            }
            continue;
        }

        out.push(ch);
    }

    for extra in values {
        out.push_str(extra);
    }

    Ok(CommandOutput::ok(out))
}

fn basename(args: &[String]) -> Result<CommandOutput> {
    let Some(raw) = args.first() else {
        return Ok(CommandOutput::error("basename: falta ruta", 2));
    };

    let normalized = raw.trim_end_matches(|ch| ch == '/' || ch == '\\');
    let name = normalized
        .rsplit(|ch| ch == '/' || ch == '\\')
        .next()
        .unwrap_or(normalized);

    Ok(CommandOutput::ok(format!("{name}\n")))
}

fn dirname(args: &[String]) -> Result<CommandOutput> {
    let Some(raw) = args.first() else {
        return Ok(CommandOutput::error("dirname: falta ruta", 2));
    };

    let normalized = raw.trim_end_matches(|ch| ch == '/' || ch == '\\');
    let position = normalized.rfind(|ch| ch == '/' || ch == '\\');

    let dir = match position {
        Some(0) => &normalized[..1],
        Some(index) => &normalized[..index],
        None => ".",
    };

    Ok(CommandOutput::ok(format!("{dir}\n")))
}

fn realpath(args: &[String], cwd: &Path) -> Result<CommandOutput> {
    let Some(raw) = args.first() else {
        return Ok(CommandOutput::error("realpath: falta ruta", 2));
    };

    let path = resolve_path(cwd, raw)
        .canonicalize()
        .with_context(|| format!("realpath: no se pudo resolver {raw}"))?;

    Ok(CommandOutput::ok(format!("{}\n", display_unix_path(&path))))
}

fn date(args: &[String]) -> Result<CommandOutput> {
    let now = Local::now();
    let format = args
        .first()
        .and_then(|value| value.strip_prefix('+'))
        .unwrap_or("%a %b %e %H:%M:%S %Y");

    Ok(CommandOutput::ok(format!("{}\n", now.format(format))))
}

fn sleep_cmd(args: &[String]) -> Result<CommandOutput> {
    let Some(value) = args.first() else {
        return Ok(CommandOutput::error("sleep: falta duración", 2));
    };

    let seconds = parse_duration(value)?;
    thread::sleep(Duration::from_secs_f64(seconds));
    Ok(CommandOutput::ok(""))
}

fn parse_duration(value: &str) -> Result<f64> {
    let (number, multiplier) = if let Some(number) = value.strip_suffix("ms") {
        (number, 0.001)
    } else if let Some(number) = value.strip_suffix('s') {
        (number, 1.0)
    } else if let Some(number) = value.strip_suffix('m') {
        (number, 60.0)
    } else if let Some(number) = value.strip_suffix('h') {
        (number, 3600.0)
    } else {
        (value, 1.0)
    };

    let parsed = number
        .parse::<f64>()
        .with_context(|| format!("sleep: duración inválida: {value}"))?;

    if parsed.is_sign_negative() {
        anyhow::bail!("sleep: la duración no puede ser negativa");
    }

    Ok(parsed * multiplier)
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


fn tar_cmd(args: &[String], cwd: &Path) -> Result<CommandOutput> {
    let mut mode: Option<char> = None;
    let mut gzip = false;
    let mut verbose = false;
    let mut archive_name: Option<String> = None;
    let mut base_dir = cwd.to_path_buf();
    let mut operands = Vec::new();
    let mut index = 0usize;

    while index < args.len() {
        let arg = &args[index];

        if arg == "-C" {
            index += 1;
            let Some(dir) = args.get(index) else {
                return Ok(CommandOutput::error("tar: -C requiere directorio", 2));
            };
            base_dir = resolve_path(cwd, dir);
            index += 1;
            continue;
        }

        let is_flag_bundle = arg.starts_with('-')
            || (index == 0
                && !arg.contains(['/', '\\', '.'])
                && arg.chars().all(|ch| matches!(ch, 'c' | 'x' | 't' | 'z' | 'v' | 'f')));

        if is_flag_bundle && arg != "-" {
            let flags = arg.trim_start_matches('-');
            let mut consumes_archive = false;

            for flag in flags.chars() {
                match flag {
                    'c' | 'x' | 't' => mode = Some(flag),
                    'z' => gzip = true,
                    'v' => verbose = true,
                    'f' => consumes_archive = true,
                    _ => {
                        return Ok(CommandOutput::error(
                            format!("tar: opción no soportada: -{flag}"),
                            2,
                        ));
                    }
                }
            }

            if consumes_archive {
                index += 1;
                let Some(file) = args.get(index) else {
                    return Ok(CommandOutput::error("tar: -f requiere archivo", 2));
                };
                archive_name = Some(file.clone());
            }

            index += 1;
            continue;
        }

        operands.push(arg.clone());
        index += 1;
    }

    let Some(mode) = mode else {
        return Ok(CommandOutput::error(
            "tar: especifique uno de -c, -x o -t",
            2,
        ));
    };
    let Some(archive_name) = archive_name else {
        return Ok(CommandOutput::error(
            "tar: esta implementación requiere -f ARCHIVO",
            2,
        ));
    };

    let archive_path = resolve_path(cwd, &archive_name);
    let gzip = gzip
        || archive_name.ends_with(".tar.gz")
        || archive_name.ends_with(".tgz");

    match mode {
        'c' => {
            if operands.is_empty() {
                return Ok(CommandOutput::error("tar: faltan archivos", 2));
            }

            let file = fs::File::create(&archive_path)?;
            if gzip {
                let encoder = GzEncoder::new(file, Compression::default());
                let mut builder = TarBuilder::new(encoder);
                append_tar_operands(&mut builder, &base_dir, &operands, verbose)?;
                let encoder = builder.into_inner()?;
                encoder.finish()?;
            } else {
                let mut builder = TarBuilder::new(file);
                append_tar_operands(&mut builder, &base_dir, &operands, verbose)?;
                builder.finish()?;
            }

            Ok(CommandOutput::ok(""))
        }
        't' => {
            let file = fs::File::open(&archive_path)?;
            let reader: Box<dyn Read> = if gzip {
                Box::new(GzDecoder::new(file))
            } else {
                Box::new(file)
            };
            let mut archive = TarArchive::new(reader);
            let mut out = String::new();

            for entry in archive.entries()? {
                let entry = entry?;
                let path = entry.path()?;
                out.push_str(&path.to_string_lossy().replace('\\', "/"));
                out.push('\n');
            }

            Ok(CommandOutput::ok(out))
        }
        'x' => {
            let file = fs::File::open(&archive_path)?;
            let reader: Box<dyn Read> = if gzip {
                Box::new(GzDecoder::new(file))
            } else {
                Box::new(file)
            };
            let mut archive = TarArchive::new(reader);
            archive.unpack(&base_dir)?;
            Ok(CommandOutput::ok(""))
        }
        _ => unreachable!(),
    }
}

fn append_tar_operands<W: Write>(
    builder: &mut TarBuilder<W>,
    base_dir: &Path,
    operands: &[String],
    verbose: bool,
) -> Result<()> {
    for operand in operands {
        let source = resolve_path(base_dir, operand);
        let archive_name = Path::new(operand);

        if !source.exists() {
            anyhow::bail!("tar: {}: no existe", source.display());
        }

        if verbose {
            println!("{}", operand.replace('\\', "/"));
        }

        if source.is_dir() {
            builder.append_dir_all(archive_name, &source)?;
        } else {
            builder.append_path_with_name(&source, archive_name)?;
        }
    }
    Ok(())
}

fn gzip_cmd(invoked_name: &str, args: &[String], cwd: &Path) -> Result<CommandOutput> {
    let decompress = invoked_name == "gunzip"
        || args.iter().any(|arg| arg == "-d" || arg == "--decompress");
    let keep = args.iter().any(|arg| arg == "-k" || arg == "--keep");
    let to_stdout = args.iter().any(|arg| arg == "-c" || arg == "--stdout");
    let files: Vec<&String> = args.iter()
        .filter(|arg| !arg.starts_with('-'))
        .collect();

    if to_stdout {
        return Ok(CommandOutput::error(
            "gzip: -c no está disponible aún porque el pipeline de SST es textual, no binario",
            2,
        ));
    }

    if files.is_empty() {
        return Ok(CommandOutput::error(
            "gzip: especifique al menos un archivo",
            2,
        ));
    }

    for file_name in files {
        let input_path = resolve_path(cwd, file_name);

        if decompress {
            let output_path = if input_path.extension().and_then(|ext| ext.to_str()) == Some("gz") {
                input_path.with_extension("")
            } else {
                return Ok(CommandOutput::error(
                    format!("gzip: {}: no termina en .gz", input_path.display()),
                    2,
                ));
            };

            let input = fs::File::open(&input_path)?;
            let mut decoder = GzDecoder::new(input);
            let mut output = fs::File::create(&output_path)?;
            io::copy(&mut decoder, &mut output)?;
        } else {
            let file_name = input_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("archivo");
            let output_path = input_path.with_file_name(format!("{file_name}.gz"));

            let mut input = fs::File::open(&input_path)?;
            let output = fs::File::create(&output_path)?;
            let mut encoder = GzEncoder::new(output, Compression::default());
            io::copy(&mut input, &mut encoder)?;
            encoder.finish()?;
        }

        if !keep {
            fs::remove_file(&input_path)?;
        }
    }

    Ok(CommandOutput::ok(""))
}

fn zip_cmd(args: &[String], cwd: &Path) -> Result<CommandOutput> {
    let recursive = args.iter().any(|arg| arg == "-r" || arg == "--recurse-paths");
    let positional: Vec<&String> = args.iter()
        .filter(|arg| !arg.starts_with('-'))
        .collect();

    let Some(archive_name) = positional.first() else {
        return Ok(CommandOutput::error("zip: falta archivo ZIP", 2));
    };
    if positional.len() < 2 {
        return Ok(CommandOutput::error("zip: faltan archivos para comprimir", 2));
    }

    let archive_path = resolve_path(cwd, archive_name);
    let file = fs::File::create(&archive_path)?;
    let mut writer = ZipWriter::new(file);

    for operand in positional.iter().skip(1) {
        let source = resolve_path(cwd, operand);
        if source.is_dir() && !recursive {
            return Ok(CommandOutput::error(
                format!("zip: {} es directorio; use -r", operand),
                2,
            ));
        }

        add_path_to_zip(&mut writer, &source, Path::new(operand))?;
    }

    writer.finish()?;
    Ok(CommandOutput::ok(""))
}

fn add_path_to_zip(
    writer: &mut ZipWriter<fs::File>,
    source: &Path,
    archive_name: &Path,
) -> Result<()> {
    let name = archive_name.to_string_lossy().replace('\\', "/");

    if source.is_dir() {
        let directory_name = if name.ends_with('/') {
            name
        } else {
            format!("{name}/")
        };
        writer.add_directory(directory_name, FileOptions::default())?;

        for entry in fs::read_dir(source)? {
            let entry = entry?;
            add_path_to_zip(
                writer,
                &entry.path(),
                &archive_name.join(entry.file_name()),
            )?;
        }
    } else {
        writer.start_file(
            name,
            FileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated),
        )?;
        let mut input = fs::File::open(source)?;
        io::copy(&mut input, writer)?;
    }

    Ok(())
}

fn unzip_cmd(args: &[String], cwd: &Path) -> Result<CommandOutput> {
    let list_only = args.iter().any(|arg| arg == "-l");
    let mut destination = cwd.to_path_buf();
    let mut archive_name: Option<&String> = None;
    let mut index = 0usize;

    while index < args.len() {
        match args[index].as_str() {
            "-d" => {
                index += 1;
                let Some(dir) = args.get(index) else {
                    return Ok(CommandOutput::error("unzip: -d requiere directorio", 2));
                };
                destination = resolve_path(cwd, dir);
            }
            "-l" | "-o" => {}
            arg if !arg.starts_with('-') && archive_name.is_none() => {
                archive_name = args.get(index);
            }
            arg if arg.starts_with('-') => {
                return Ok(CommandOutput::error(
                    format!("unzip: opción no soportada: {arg}"),
                    2,
                ));
            }
            _ => {}
        }
        index += 1;
    }

    let Some(archive_name) = archive_name else {
        return Ok(CommandOutput::error("unzip: falta archivo ZIP", 2));
    };

    let file = fs::File::open(resolve_path(cwd, archive_name))?;
    let mut archive = ZipArchive::new(file)?;
    let mut out = String::new();

    if list_only {
        for index in 0..archive.len() {
            let entry = archive.by_index(index)?;
            out.push_str(entry.name());
            out.push('\n');
        }
        return Ok(CommandOutput::ok(out));
    }

    fs::create_dir_all(&destination)?;

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let Some(relative) = entry.enclosed_name().map(Path::to_path_buf) else {
            return Ok(CommandOutput::error(
                format!("unzip: ruta insegura en archivo: {}", entry.name()),
                2,
            ));
        };
        let output_path = destination.join(relative);

        if entry.is_dir() {
            fs::create_dir_all(&output_path)?;
            continue;
        }

        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut output = fs::File::create(&output_path)?;
        io::copy(&mut entry, &mut output)?;
    }

    Ok(CommandOutput::ok(""))
}


fn which(args: &[String]) -> Result<CommandOutput> {
    let Some(name) = args.first() else {
        return Ok(CommandOutput::error("which: falta comando", 2));
    };

    if is_internal_command(name) {
        return Ok(CommandOutput::ok(format!(
            "{name}: comando interno de Shell Shock Tool\n"
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

fn is_internal_command(name: &str) -> bool {
    matches!(
        name,
        "cd"
            | "clear"
            | "exit"
            | "logout"
            | "vim"
            | "edit"
            | "alias"
            | "unalias"
            | "export"
            | "history"
            | "config"
            | "help"
            | "man"
            | "net"
            | "sys"
            | "domain"
            | "device"
            | "switch"
            | "wol"
            | "diag"
            | "pwd"
            | "echo"
            | "env"
            | "ls"
            | "cat"
            | "head"
            | "tail"
            | "grep"
            | "wc"
            | "sort"
            | "uniq"
            | "cut"
            | "xargs"
            | "tar"
            | "gzip"
            | "gunzip"
            | "zip"
            | "unzip"
            | "tee"
            | "less"
            | "more"
            | "sed"
            | "awk"
            | "diff"
            | "sha256sum"
            | "base64"
            | "find"
            | "printf"
            | "basename"
            | "dirname"
            | "realpath"
            | "date"
            | "sleep"
            | "true"
            | "false"
            | "touch"
            | "mkdir"
            | "rm"
            | "cp"
            | "mv"
            | "which"
            | "type"
            | "ps"
            | "top"
            | "df"
            | "free"
            | "hostname"
            | "whoami"
            | "uname"
            | "kill"
    )
}


#[cfg(test)]
mod archive_tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = env::temp_dir().join(format!(
            "sst-{name}-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn archive_utilities_round_trip() {
        let root = temp_root("archives");
        fs::write(root.join("alpha.txt"), "uno\ndos\n").unwrap();
        fs::create_dir_all(root.join("folder")).unwrap();
        fs::write(root.join("folder").join("beta.txt"), "tres\n").unwrap();

        tar_cmd(
            &[
                "-czf".into(),
                "bundle.tar.gz".into(),
                "alpha.txt".into(),
                "folder".into(),
            ],
            &root,
        )
        .unwrap();
        let listing = tar_cmd(
            &["-tzf".into(), "bundle.tar.gz".into()],
            &root,
        )
        .unwrap();
        assert!(listing.stdout.contains("alpha.txt"));
        assert!(listing.stdout.contains("folder/beta.txt"));

        let tar_out = root.join("tar-out");
        fs::create_dir_all(&tar_out).unwrap();
        tar_cmd(
            &[
                "-xzf".into(),
                "bundle.tar.gz".into(),
                "-C".into(),
                "tar-out".into(),
            ],
            &root,
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(tar_out.join("alpha.txt")).unwrap(),
            "uno\ndos\n"
        );

        zip_cmd(
            &[
                "-r".into(),
                "bundle.zip".into(),
                "alpha.txt".into(),
                "folder".into(),
            ],
            &root,
        )
        .unwrap();
        let zip_listing = unzip_cmd(
            &["-l".into(), "bundle.zip".into()],
            &root,
        )
        .unwrap();
        assert!(zip_listing.stdout.contains("alpha.txt"));
        assert!(zip_listing.stdout.contains("folder/beta.txt"));

        let zip_out = root.join("zip-out");
        unzip_cmd(
            &[
                "bundle.zip".into(),
                "-d".into(),
                "zip-out".into(),
            ],
            &root,
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(zip_out.join("folder").join("beta.txt")).unwrap(),
            "tres\n"
        );

        fs::write(root.join("gamma.txt"), "contenido gzip").unwrap();
        gzip_cmd(
            "gzip",
            &["-k".into(), "gamma.txt".into()],
            &root,
        )
        .unwrap();
        assert!(root.join("gamma.txt.gz").is_file());
        fs::remove_file(root.join("gamma.txt")).unwrap();

        gzip_cmd(
            "gunzip",
            &["-k".into(), "gamma.txt.gz".into()],
            &root,
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(root.join("gamma.txt")).unwrap(),
            "contenido gzip"
        );

        fs::remove_dir_all(root).unwrap();
    }
}
