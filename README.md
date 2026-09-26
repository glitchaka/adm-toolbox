# ADM Toolbox

ADM Toolbox es una consola portable de soporte técnico para Windows escrita íntegramente en Rust.

La aplicación está diseñada para sentirse como una terminal Linux/Bash durante toda la sesión: prompt, historial, comandos, pipes, redirecciones, aliases, variables, scripts y utilidades de administración invocadas desde la propia shell.

## Compilar

```powershell
cargo build --release
```

El ejecutable queda en:

```text
target\release\adm-toolbox.exe
```

## Shell

Ejemplos:

```bash
pwd
ls -la
cd /c/Users/manuel
cat archivo.txt | grep -i error | sort
netstat -ano | grep LISTENING
alias ll='ls -la'
export TEST=hola
source script.sh
```

ADM Toolbox no usa cmd.exe como interfaz. Puede lanzar programas .exe de Windows directamente desde la shell.

## Administración

```bash
sys info
sys processes
sys disks
sys memory

net interfaces
net connections
net neighbors
net scan
net scan 192.168.1.0/24
net ports 192.168.1.25 22,80,443,3389
net traffic
net usage
net provider capabilities

domain status
wol AA:BB:CC:DD:EE:FF

device add AA:BB:CC:DD:EE:FF LAB-PC-01
device list

switch capabilities
switch locate AA:BB:CC:DD:EE:FF
```

`net traffic` ya correlaciona conexiones activas con PID, proceso, CPU y RAM. La medición exacta de bytes/s por proceso está reservada para el proveedor ETW de la siguiente etapa.

`net usage` requiere un router, firewall, AP o controlador que entregue contadores por cliente. ADM Toolbox no inventará consumo si el gateway no expone esos datos.

## Editor tipo Vim escrito en Rust

El comando:

```bash
vim archivo.conf
```

abre un editor modal implementado dentro de ADM Toolbox, también en Rust. `edit` es un alias del mismo editor.

No se incrusta vim.exe ni código C del Vim original. El objetivo es conservar el modelo de interacción de Vim sin romper el requisito de implementación Rust.

Funciones iniciales:

- modos Normal, Insert, Command y Search;
- `h`, `j`, `k`, `l` y flechas;
- `w`, `b`, `0`, `$`, `gg`, `G`;
- `i`, `a`, `o`, `O`;
- `x`, `dd`, `yy`, `p`;
- `u` y `Ctrl-R`;
- `/texto`, `n`, `N`;
- `:w`, `:q`, `:q!`, `:wq`, `:x`.

## Arquitectura

```text
ADM Toolbox
├── Shell
│   ├── parser
│   ├── aliases / env / history
│   ├── pipes / redirect
│   └── external Windows executables
├── Unix commands
├── ADM commands
│   ├── sys
│   ├── net
│   ├── domain
│   ├── wol
│   ├── device
│   └── switch
└── Vim-like editor (Rust)
```

El alcance funcional completo está en `ADM_TOOLBOX_PLAN.md`.
