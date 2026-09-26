mod device;
mod diag;
mod domain;
mod net;
mod switch;
#[cfg(windows)]
mod traffic_etw;
#[cfg(windows)]
mod win_process;
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
            | "man"
            | "pwd"
            | "echo"
            | "env"
            | "clear"
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
            | "sys"
            | "net"
            | "domain"
            | "wol"
            | "diag"
            | "device"
            | "switch"
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

pub fn run(
    name: &str,
    args: &[String],
    input: Option<&[u8]>,
    cwd: &Path,
) -> Result<CommandOutput> {
    if args.first().is_some_and(|arg| arg == "--help" || arg == "-h") {
        return Ok(help(&[name.to_owned()]));
    }

    match name {
        "help" | "man" => Ok(help(args)),
        "pwd" | "echo" | "env" | "clear" | "ls" | "cat" | "head" | "tail" | "grep" | "wc" | "sort"
        | "uniq" | "cut" | "tee" | "less" | "more" | "sed" | "awk" | "diff"
        | "sha256sum" | "base64" | "find" | "printf" | "basename" | "dirname"
        | "realpath" | "date" | "sleep" | "true" | "false" | "touch" | "mkdir" | "rm"
        | "cp" | "mv" | "which" | "type" => unix::run(name, args, input, cwd),
        "sys" => sys::run(args),
        "net" => net::run(args),
        "domain" => domain::run(args),
        "wol" => wol::run(args),
        "diag" => diag::run(args),
        "device" => device::run(args),
        "switch" => switch::run(args),
        "ps" | "top" | "df" | "free" | "hostname" | "whoami" | "uname" | "kill" => {
            sys::run_alias(name, args)
        }
        _ => Ok(CommandOutput::error(
            format!("comando interno desconocido: {name}"),
            127,
        )),
    }
}

fn help(args: &[String]) -> CommandOutput {
    match args.first().map(String::as_str) {
        Some("net") => CommandOutput::ok(
            "net — red y diagnóstico\n\n\
net interfaces                 configuración de interfaces\n\
net connections                conexiones TCP/UDP\n\
net routes                     tabla de rutas\n\
net neighbors                  vecinos ARP\n\
net dns HOST|IP                resolución directa/inversa\n\
net ping HOST [-c N]            ping\n\
net trace HOST                 traceroute\n\
net scan [CIDR]                descubre equipos\n\
net monitor                    monitor interactivo de la LAN\n\
net ports HOST [LISTA]         puertos TCP acotados\n\
net traffic [--watch]          procesos con conexiones de red\n\
net usage                      uso por cliente mediante proveedor de gateway\n\
net provider capabilities      capacidades del proveedor\n"
        ),
        Some("sys") => CommandOutput::ok(
            "sys — información y procesos\n\n\
sys info                       resumen del equipo\n\
sys processes                  procesos\n\
sys top                        monitor interactivo\n\
sys disks                      discos\n\
sys memory                     memoria\n\
sys hostname                   nombre del equipo\n\
sys whoami                     usuario actual\n\
sys uname [-a]                 información de sistema\n\
sys kill PID                   termina un proceso\n\n\
Aliases: ps, top, df, free, hostname, whoami, uname, kill\n"
        ),
        Some("vim") | Some("edit") => CommandOutput::ok(
            "vim FILE — editor modal Rust\n\n\
Normal: h j k l, w, b, 0, $, gg, G, i, a, o, O, x, dd, yy, p, u, Ctrl-R\n\
Visual línea: V, j/k, y, d\n\
Buscar: /texto, n, N\n\
Ex: :w, :q, :q!, :wq, :x, :set number, :set nonumber, :%s/a/b/g\n"
        ),
        Some("device") => CommandOutput::ok(
            "device — inventario local\n\n\
device list\n\
device add MAC NOMBRE\n\
device remove MAC\n"
        ),
        Some("domain") => CommandOutput::ok(
            "domain — pertenencia a dominio\n\n\
domain status [HOST]\n"
        ),
        Some("switch") => CommandOutput::ok(
            "switch — resolución de infraestructura\n\n\
switch capabilities\n\
switch locate MAC\n"
        ),
        Some(topic) => CommandOutput::error(format!("help: tema desconocido: {topic}"), 1),
        None => CommandOutput::ok(
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
  config path|edit|reload     configuración portable admrc\n\
  exit                       sale de ADM Toolbox\n\
  vim FILE                   editor modal Rust\n\
  edit FILE                  alias de vim\n\n\
Unix:\n\
  ls, cat, head, tail, grep, wc, sort, uniq, cut, tee, find\n\
  less, sed, awk, diff, sha256sum, base64\n\
  printf, basename, dirname, realpath, date, sleep, true, false\n\
  touch, mkdir, rm, cp, mv, which, type\n\n\
Sistema:\n\
  ps, top, df, free, hostname, whoami, uname, kill\n\n\
Administración:\n\
  sys ...                    usa 'help sys'\n\
  net ...                    usa 'help net'\n\
  domain ...                 usa 'help domain'\n\
  device ...                 usa 'help device'\n\
  switch ...                 usa 'help switch'\n\
  wol MAC [BROADCAST]\n\
  diag network|traffic\n\n\
Operadores:\n\
  cmd1 | cmd2\n\
  cmd < archivo\n\
  cmd > archivo\n\
  cmd >> archivo\n\
  cmd1 && cmd2\n\
  cmd1 || cmd2\n\
  cmd1 ; cmd2\n\n\
Usa 'man TEMA' o 'help TEMA' para ayuda detallada.\n"
        ),
    }
}
