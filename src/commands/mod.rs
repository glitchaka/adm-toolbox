mod device;
mod diag;
mod domain;
mod net;
mod switch;
mod sys;
mod unix;
mod wol;

use std::path::Path;

use anyhow::Result;

pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub status: i32,
}

impl CommandOutput {
    pub fn ok(stdout: impl Into<String>) -> Self {
        Self {
            stdout: stdout.into(),
            stderr: String::new(),
            status: 0,
        }
    }

    pub fn error(stderr: impl Into<String>, status: i32) -> Self {
        Self {
            stdout: String::new(),
            stderr: stderr.into(),
            status,
        }
    }
}

pub fn is_internal(name: &str) -> bool {
    matches!(
        name,
        "help"
            | "pwd"
            | "echo"
            | "ls"
            | "cat"
            | "head"
            | "tail"
            | "grep"
            | "wc"
            | "sort"
            | "uniq"
            | "cut"
            | "tee"
            | "find"
            | "touch"
            | "mkdir"
            | "rm"
            | "cp"
            | "mv"
            | "which"
            | "sys"
            | "net"
            | "domain"
            | "wol"
            | "diag"
            | "device"
            | "switch"
    )
}

pub fn run(
    name: &str,
    args: &[String],
    input: Option<&[u8]>,
    cwd: &Path,
) -> Result<CommandOutput> {
    match name {
        "help" => Ok(help()),
        "pwd" | "echo" | "ls" | "cat" | "head" | "tail" | "grep" | "wc" | "sort"
        | "uniq" | "cut" | "tee" | "find" | "touch" | "mkdir" | "rm" | "cp" | "mv"
        | "which" => unix::run(name, args, input, cwd),
        "sys" => sys::run(args),
        "net" => net::run(args),
        "domain" => domain::run(args),
        "wol" => wol::run(args),
        "diag" => diag::run(args),
        "device" => device::run(args),
        "switch" => switch::run(args),
        _ => Ok(CommandOutput::error(
            format!("comando interno desconocido: {name}"),
            127,
        )),
    }
}

fn help() -> CommandOutput {
    CommandOutput::ok(
        "ADM Toolbox\n\n\
Shell:\n\
  cd PATH                    cambia de directorio\n\
  pwd                        muestra el directorio actual\n\
  clear                      limpia la terminal\n\
  history                    historial de esta sesión\n\
  alias [N=COMANDO]          crea/lista aliases\n\
  unalias N                  elimina un alias\n\
  export N=VALOR             define variable de entorno\n\
  source FILE                ejecuta un script\n\
  exit                       sale de ADM Toolbox\n\
  vim FILE                   editor modal Rust\n\
  edit FILE                  alias de vim\n\n\
Unix:\n\
  ls [-la] [PATH]\n\
  cat FILE...\n\
  head [-n N] [FILE]\n\
  tail [-n N] [FILE]\n\
  grep [-in] PATTERN [FILE]\n\
  wc [FILE]\n\
  sort [FILE]\n\
  uniq [FILE]\n\
  cut -d DELIM -f N [FILE]\n\
  tee [-a] FILE\n\
  find [PATH] [-name PATRON]\n\
  touch FILE...\n\
  mkdir [-p] DIR...\n\
  rm [-r] PATH...\n\
  cp SOURCE TARGET\n\
  mv SOURCE TARGET\n\
  which COMMAND\n\
  echo TEXT...\n\n\
Administración:\n\
  sys info\n\
  sys processes\n\
  sys disks\n\
  sys memory\n\
  net interfaces\n\
  net connections\n\
  net neighbors\n\
  net scan [CIDR]\n\
  net ports HOST PORTS\n\
  net traffic\n\
  net usage\n\
  net provider capabilities\n\
  domain status [HOST]\n\
  wol MAC [BROADCAST]\n\
  diag network\n\
  device list|add|remove\n\
  switch capabilities|locate\n\n\
También ejecuta directamente programas de Windows como ipconfig, ping, netstat, tasklist y powershell.\n"
    )
}
