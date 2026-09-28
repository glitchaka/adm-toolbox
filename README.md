# Shell Shock Tool (SST)

**Shell Shock Tool**, o **SST**, es una consola portable para Windows escrita en Rust. Combina una terminal Win32 propia, un intérprete Bash-compatible, utilidades Unix integradas y herramientas de soporte técnico, diagnóstico, inventario y red.

El ejecutable actual se llama:

```text
sst.exe
```

SST está pensado para poder llevarse como herramienta portable y trabajar desde una única consola sin depender de PowerShell para las tareas que ya incorpora de forma nativa.

---

## Construcción

```powershell
cargo build --release
```

El ejecutable se genera en:

```text
target\release\sst.exe
```

Durante la compilación, `build.rs` incorpora la Nerd Font y el paquete oficial de Helix utilizados por la aplicación. Para compilaciones sin Internet pueden definirse:

```text
SST_NERD_FONT_FILE
SST_HELIX_ARCHIVE
```

No es necesario distribuir los directorios internos de Cargo como `build/`, `deps/`, `incremental/` o los archivos `.pdb`/`.d` para ejecutar SST.

---

# Mapa de herramientas

SST tiene cuatro grupos principales de herramientas:

1. **Shell Bash-compatible**: scripting, variables, pipes, redirecciones, jobs, arrays, funciones, traps, historial y completion.
2. **Utilidades Unix integradas**: archivos, texto, compresión, hashes, búsqueda y navegación.
3. **Herramientas SST para Windows y red**: sistema, red, inventario, Wake-on-LAN, dominio, switches, tráfico y diagnósticos.
4. **Herramientas auxiliares**: configuración portable, traducción de rutas y editor Helix integrado.

Puedes obtener ayuda desde la propia shell con:

```bash
help
help COMANDO
```

---

# Herramientas SST

## `sys` — sistema y administración local

`sys` agrupa consultas del equipo y herramientas de auditoría local.

### Información del equipo

| Comando | Qué hace |
|---|---|
| `sys info` | Muestra un resumen del sistema operativo, CPU, memoria y equipo. |
| `sys processes` | Lista procesos y su consumo de CPU/RAM. |
| `sys top` | Abre un monitor TUI de procesos. Permite ordenar por CPU o memoria. |
| `sys disks` | Muestra discos, capacidad y uso. |
| `sys memory` | Muestra memoria física y swap. |
| `sys uptime` | Indica cuánto tiempo lleva Windows desde el último arranque. |
| `sys hostname` | Muestra el nombre del equipo. |
| `sys whoami` | Muestra el usuario actual. |
| `sys uname [-a]` | Muestra identificación del sistema en formato familiar para usuarios Unix. |
| `sys kill PID` | Termina un proceso por PID. |
| `sys fetch [--small|--full]` | Muestra información del sistema acompañada por la mascota de SST. |

También existen como comandos directos:

```text
ps
top
df
free
uptime
hostname
whoami
uname
kill
fetch
neofetch
fastfetch
```

### Servicios de Windows

```bash
sys services
sys services --running
sys services --stopped
sys services NOMBRE
```

Permite listar servicios, filtrar por estado o consultar un servicio concreto. Usa `sc.exe` como backend y actualmente es de consulta.

### Usuarios

```bash
sys users
sys users USUARIO
sys users --domain
sys users USUARIO --domain
```

Consulta cuentas locales o del dominio mediante las herramientas nativas de Windows.

### Drivers y dispositivos PnP

```bash
sys drivers
sys drivers --verbose
sys drivers --signed
sys drivers --csv
sys drivers --pnp
sys drivers --devices
```

- `--verbose`: información detallada de los drivers.
- `--signed`: añade información de firma.
- `--csv`: salida CSV.
- `--pnp`: enumera paquetes de drivers PnP.
- `--devices`: enumera dispositivos PnP conectados.

### Windows Event Log

```bash
sys events
sys events Application
sys events --log Security
sys events --count 50
sys events --query XPATH
sys events --format text
sys events --format xml
sys events --logs
sys events --publishers
```

Permite consultar eventos recientes, elegir log, limitar cantidad, aplicar filtros XPath, obtener XML y enumerar logs o publishers.

### Registro de Windows

```bash
sys registry "HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion"
sys registry CLAVE --value NOMBRE
sys registry CLAVE --default
sys registry CLAVE --recursive
sys registry CLAVE --find TEXTO
sys registry CLAVE --find TEXTO --keys
sys registry CLAVE --find TEXTO --data
```

La integración actual es **de solo lectura** y utiliza `reg.exe query`.

### Tareas programadas

```bash
sys tasks
sys tasks --verbose
sys tasks --csv
sys tasks NOMBRE
sys tasks NOMBRE --xml
```

Consulta tareas programadas, detalles o su definición XML mediante `schtasks.exe`.

---

## `net` — diagnóstico, descubrimiento y tráfico de red

### Interfaces

```bash
net interfaces
```

Muestra las interfaces de red del equipo.

### Conexiones

```bash
net connections
```

Muestra protocolo, dirección local, dirección remota, estado y PID de las conexiones activas.

### Rutas

```bash
net routes
```

Muestra la tabla de rutas de Windows.

### DNS

```bash
net dns HOST
net dns IP
```

Resuelve nombres a direcciones IP y realiza resolución inversa cuando se entrega una IP.

### Ping

```bash
net ping HOST
net ping HOST -c 10
```

Envía ICMP desde el motor nativo de SST. `-c` acepta entre 1 y 100 intentos.

### Trace

```bash
net trace HOST
net traceroute HOST
```

Realiza un trazado ICMP de hasta 30 saltos.

### Vecinos / ARP

```bash
net neighbors
net arp
```

Muestra la relación IP → MAC conocida por el equipo.

### Escaneo de puertos

```bash
net ports HOST
net ports HOST 22,80,443,445,3389
```

Comprueba conectividad TCP contra los puertos indicados. Si no se especifican puertos usa un conjunto habitual.

### Descubrimiento de red

```bash
net scan
net scan 192.168.1.0/24
net scan --unknown
net scan --authorized
net scan --known
net scan --names
net scan --json
net scan --csv
```

Escanea una red IPv4 y relaciona los equipos encontrados con:

- dirección IP;
- nombre del equipo;
- MAC;
- latencia;
- estado conocido/desconocido;
- nombre del inventario SST, si existe.

La salida normal prioriza las columnas **IP** y **NOMBRE**. SST intenta resolver el hostname del equipo y, si no hay resolución disponible pero el dispositivo está inventariado, utiliza el nombre guardado en el inventario.

Sin red explícita, SST intenta determinar la red IPv4 local y usa una /24. Por seguridad, el escaneo está limitado a redes /20 o más pequeñas.

`--unknown` muestra solo equipos que no están en el inventario.  
`--authorized` y `--known` muestran solo equipos conocidos.  
`--names` muestra únicamente IP y nombre del equipo, como un IP Scanner simple:

```text
IP               NOMBRE
192.168.1.10     PC-RECEPCION
192.168.1.21     NOTEBOOK-01
```

### Monitor de presencia

```bash
net monitor
net monitor 192.168.1.0/24
net monitor --unknown
```

Abre una vista TUI que repite el descubrimiento y registra:

- aparición de equipos;
- desaparición;
- cambios de IP;
- primera y última vez vistos.

Se sale con `q` o `Esc`.

### Historial de presencia

```bash
net presence
net presence --json
net presence --csv
```

Consulta el historial persistente generado por `net monitor`.

### Tráfico por proceso

```bash
net traffic
net traffic --watch
net traffic --top 20
net traffic --pid 4120
net traffic --process chrome
net traffic --background
net traffic --high-usage
net traffic --connections
net traffic --json
net traffic --csv
```

Correlaciona:

- PID y PPID;
- nombre del proceso;
- proceso en primer plano;
- ejecutable;
- CPU;
- RAM;
- número de conexiones;
- subida y bajada por segundo.

La medición de bytes por PID utiliza ETW de Windows. Si ETW no está disponible, SST lo indica explícitamente en vez de inventar datos.

`--connections` cambia la vista para mostrar protocolo, extremos local/remoto, estado, PID y proceso.

`--watch` abre una vista TUI actualizada periódicamente y se cierra con `q` o `Esc`.

### Proveedores de red

```bash
net provider list
net provider list --json
net provider add NOMBRE --type TIPO --host HOST
net provider use NOMBRE
net provider current
net provider remove NOMBRE
net provider capabilities
net provider capabilities NOMBRE
net provider path
```

Permite registrar routers, firewalls, controladores o fuentes externas de telemetría.

Tipos contemplados por la capa de capacidades:

```text
openwrt
opnsense
pfsense
unifi
snmp
generic
```

Esta capa guarda la configuración del proveedor y describe qué integración sería posible.

### Uso de Internet de toda la LAN

```bash
net usage
```

**Estado actual: pendiente.** La infraestructura para elegir un proveedor existe, pero todavía no está implementado el driver que obtiene contadores por cliente desde OpenWrt, OPNsense, pfSense, UniFi u otro equipo. SST no intenta inferir esos datos a partir de ARP o ping.

---

## `device` — inventario local de equipos

```bash
device list
device list --json
device list --csv
device show MAC
device show NOMBRE
device add AA:BB:CC:DD:EE:FF NOMBRE
device add AA:BB:CC:DD:EE:FF NOMBRE --note "texto"
device remove AA:BB:CC:DD:EE:FF
device unknown
device unknown --json
device unknown --csv
device path
```

El inventario guarda MAC, nombre y notas. Se utiliza para identificar dispositivos durante escaneos, Wake-on-LAN y localización en switches.

`device unknown` cruza el historial de presencia generado por `net monitor` con el inventario local y muestra únicamente los equipos detectados cuya MAC todavía no está registrada. La salida incluye IP, hostname, MAC, primera detección y última detección; también puede exportarse con `--json` o `--csv`.

No vuelve a escanear la red por su cuenta: trabaja sobre observaciones persistidas por SST, de modo que sirve para revisar posteriormente qué equipos aparecieron en una red administrada.

---

## `wol` — Wake-on-LAN

```bash
wol AA:BB:CC:DD:EE:FF
wol NOMBRE-INVENTARIADO
wol NOMBRE-INVENTARIADO 192.168.1.255:9
```

Envía un Magic Packet. El destino puede ser una MAC directa o el nombre de un equipo registrado con `device add`.

Si no se indica broadcast utiliza:

```text
255.255.255.255:9
```

---

## `domain` — estado de dominio

```bash
domain status
domain status EQUIPO
domain status EQUIPO --verify
domain status EQUIPO --cim
domain status EQUIPO --json
```

Para el equipo local consulta la pertenencia al dominio. Para equipos remotos puede obtener información básica y, con `--verify`/`--cim`, solicitar una comprobación más explícita.

La salida distingue:

- hostname;
- dominio;
- si está unido o no;
- fuente de la información;
- nivel de confianza;
- logon server.

---

## `switch` — MAC → switch → puerto físico

### Configuración

```bash
switch add SW-PISO2 --host 10.0.0.12 --community-env SW_CORE_COMMUNITY
switch add SW-PISO2 --host 10.0.0.12 --community-env SW_CORE_COMMUNITY --description "Core piso 2"
switch list
switch list --json
switch show SW-PISO2
switch remove SW-PISO2
switch path
switch capabilities
```

La comunidad SNMP **no se guarda en texto plano**. El perfil guarda el nombre de una variable de entorno:

```bash
export SW_CORE_COMMUNITY='comunidad-snmp'
```

### Localización

```bash
switch locate AA:BB:CC:DD:EE:FF
switch locate NOMBRE-INVENTARIADO
switch locate NOMBRE --switch SW-PISO2
switch locate NOMBRE --vlan 20
switch locate NOMBRE --json
```

SST consulta el switch por SNMP de solo lectura y resuelve, cuando el equipo lo permite:

```text
MAC
  ↓
switch
  ↓
bridge port
  ↓
ifIndex
  ↓
interfaz física
```

La salida puede incluir VLAN, PVID, alias, velocidad y estado operativo. La implementación utiliza Bridge-MIB, Q-BRIDGE-MIB e IF-MIB.

---

## `diag` — diagnósticos compuestos

```bash
diag network
diag dns
diag dns HOST
diag hardware
diag storage
diag traffic
diag domain
```

Agrupa varias herramientas para obtener una vista rápida:

- `diag network`: interfaces, vecinos y conexiones.
- `diag dns`: prueba resolución DNS; usa `example.com` si no se especifica destino.
- `diag hardware`: información general, memoria y discos.
- `diag storage`: estado de discos.
- `diag traffic`: tráfico de red por proceso.
- `diag domain`: pertenencia al dominio local.

---

# Utilidades Unix integradas

Estas herramientas están escritas o integradas dentro de SST y no requieren instalar GNU coreutils. La intención es ofrecer una experiencia familiar, no afirmar equivalencia completa con cada implementación GNU original.

| Comando | Función |
|---|---|
| `pwd` | Muestra el directorio actual. |
| `echo` | Imprime argumentos. |
| `env` | Lista variables de entorno. |
| `clear` | Limpia la terminal. |
| `ls` | Lista archivos y directorios. |
| `cat` | Concatena archivos o stdin. |
| `head` | Muestra las primeras líneas. |
| `tail` | Muestra las últimas líneas. |
| `grep` | Filtra líneas por texto. |
| `wc` | Cuenta líneas, palabras y bytes. |
| `sort` | Ordena líneas. |
| `uniq` | Elimina líneas adyacentes repetidas. |
| `cut` | Selecciona campos. |
| `xargs` | Construye y ejecuta comandos a partir de stdin. |
| `tee` | Copia stdin simultáneamente a archivo y stdout. |
| `less` | Paginador interactivo. |
| `more` | Alias del paginador. |
| `sed` | Sustitución de texto. |
| `awk` | Selección simple de campos. |
| `diff` | Compara dos archivos. |
| `printf` | Imprime texto con formato. |
| `find` | Busca archivos. |
| `basename` | Extrae el último componente de una ruta. |
| `dirname` | Extrae el directorio de una ruta. |
| `realpath` | Resuelve una ruta absoluta. |
| `date` | Muestra fecha y hora. |
| `sleep` | Espera un intervalo. |
| `true` | Termina con estado 0. |
| `false` | Termina con estado 1. |
| `touch` | Crea un archivo vacío o actualiza su presencia. |
| `mkdir` | Crea directorios. |
| `rm` | Elimina archivos o directorios. |
| `cp` | Copia archivos. |
| `mv` | Mueve o renombra archivos. |
| `tar` | Crea, lista y extrae TAR/TAR.GZ. |
| `gzip` | Comprime archivos con gzip. |
| `gunzip` | Descomprime archivos `.gz`. |
| `zip` | Crea archivos ZIP. |
| `unzip` | Lista o extrae ZIP. |
| `sha256sum` | Calcula SHA-256. |
| `base64` | Codifica o decodifica Base64. |
| `which` | Localiza un comando ejecutable. |
| `type` | Indica cómo se resolverá un nombre de comando. |

## Programas externos disponibles desde SST

Además de sus herramientas integradas, SST puede lanzar ejecutables instalados en Windows o disponibles en `PATH`. Por ejemplo:

```bash
ipconfig /all
ping 8.8.8.8
netstat -ano
tasklist
systeminfo
powershell
ssh
curl
```

Estos programas **no forman parte de SST**; la shell simplemente los ejecuta como comandos externos.

---

# Configuración portable

El archivo de configuración actual es:

```text
config/sstrc
```

Usa sintaxis Bash-compatible.

### Comando amigable

```bash
config path
config edit
config reload
```

- `config path`: muestra la ruta del archivo.
- `config edit`: lo abre con el editor integrado.
- `config reload`: vuelve a ejecutar el archivo sin reiniciar SST.

### Builtin interno

```bash
sst-config path
sst-config edit
```

La variable que contiene la ruta es:

```text
SST_CONFIG
```

---

# Traducción de rutas

```bash
sst-path /c/Windows/System32
```

Convierte rutas estilo Unix a rutas Windows.

SST también muestra rutas del prompt de forma familiar:

```text
C:\Users\usuario       → ~
C:\Windows\System32    → /c/Windows/System32
```

---

# Editor integrado: helix-sst

Comandos registrados:

```bash
helix archivo.txt
hx archivo.txt
helix-sst archivo.txt
helix --version
helix --credits
helix --help
```

La distribución portable está basada en **Helix 25.07.1** y SST aporta:

- empaquetado dentro de la aplicación;
- configuración portable;
- tema propio;
- puente PTY/ConPTY;
- integración de clipboard;
- transporte VT;
- integración con la terminal nativa.

### Estado actual

**helix-sst está funcional dentro de SST.**

El editor puede iniciarse y utilizarse de forma interactiva desde la terminal nativa, incluyendo edición mediante teclado, navegación y guardado a través del puente PTY/ConPTY integrado.

El log queda en:

```text
config/helix-sst/helix.log
```

---

# Estado de Bash

SST **no ejecuta un Bash externo**. Tiene un intérprete propio escrito en Rust con objetivo de compatibilidad con **Bash 5.3**.

No debe confundirse “compatible con Bash” con “GNU Bash recompilado para Windows”: SST implementa la sintaxis y semántica dentro de su propio motor y adapta al modelo de procesos y archivos de Windows las partes que dependen de Unix.

## Lenguaje implementado

Actualmente están implementados:

- comandos simples y secuencias;
- `&&`, `||` y `!`;
- pipes `|` y `|&`;
- pipelines paralelos;
- ejecución en background con `&`;
- grupos `{ ...; }`;
- subshells `(...)`;
- `if / elif / else / fi`;
- `for`;
- `for ((...))`;
- `while`;
- `until`;
- `select`;
- `case` con `;;`, `;&` y `;;&`;
- funciones;
- `[[ ... ]]`;
- `(( ... ))`;
- `time`;
- `coproc`;
- asignaciones simples y arrays;
- variables locales y scopes;
- parámetros posicionales `$0`, `$1`, `$@`, `$*`, `$#`;
- arrays indexados;
- arrays asociativos;
- namerefs;
- arrays dispersos;
- atributos de variables;
- funciones exportadas.

## Expansiones

Incluye:

- expansión de variables;
- valores por defecto y operadores de parámetros;
- sustitución de comandos `$(...)`;
- expansión aritmética `$((...))`;
- brace expansion;
- tilde expansion;
- globbing;
- `extglob`;
- `globstar`;
- `GLOBIGNORE`;
- `GLOBSORT`;
- `dotglob`;
- `nullglob`;
- `failglob`;
- `nocaseglob`;
- IFS;
- quoting simple y doble;
- quoting ANSI-C;
- sustituciones modernas de Bash 5.3 ejecutadas en el shell actual.

## Redirecciones

Se implementan:

- `>`;
- `>>`;
- `<`;
- `<>`;
- `2>` y otros descriptores;
- duplicación `2>&1`;
- cierre de descriptores;
- `&>`;
- `&>>`;
- heredocs;
- here-strings;
- descriptores asignados a variables, por ejemplo `{fd}>archivo`;
- redirecciones sobre comandos compuestos.

## Process substitution y coprocesos

```bash
diff <(comando1) <(comando2)
coproc mi_proceso { comando; }
```

SST implementa process substitution y coprocesos usando mecanismos compatibles con Windows.

## Job control

Implementado:

```text
jobs
fg
bg
wait
wait -n
disown
kill
```

También reconoce job specs como `%1`, `%+`, `%-` y búsquedas por nombre.

El control de jobs está adaptado a procesos y threads de Windows, por lo que no existe un controlling TTY POSIX idéntico al de Linux.

## Scripts `.sh`

SST reconoce scripts Bash/sh:

```bash
test.sh
./test.sh
sst.exe test.sh
```

En el estado actual de `main`, un script local Bash/sh puede ejecutarse directamente dentro del intérprete activo. Esto evita relanzar otra instancia de SST y permite que `read` utilice el transporte de entrada de la terminal actual.

SST acepta deliberadamente un `.sh` del directorio actual por nombre, por ejemplo `test.sh`.

## Builtins Bash disponibles

```text
:
.
[
alias
bg
bind
break
builtin
caller
cd
command
compgen
complete
compopt
continue
declare
dirs
disown
echo
enable
eval
exec
exit
export
false
fc
fg
getopts
hash
help
history
jobs
kill
let
local
logout
mapfile
popd
printf
pushd
pwd
read
readarray
readonly
return
set
shift
shopt
source
suspend
test
times
trap
true
type
typeset
ulimit
umask
unalias
unset
wait
```

Entre las funciones relevantes se encuentran:

- `read -e` y `read -E`;
- `source -p`;
- `trap -P`;
- `compgen -V`;
- programmable completion con `complete`, `compgen` y `compopt`;
- historial con `history` y `fc`;
- bindings con `bind`;
- `mapfile`/`readarray`;
- `getopts`;
- `hash`;
- pila de directorios con `dirs`, `pushd`, `popd`.

## Variables especiales

SST implementa, entre otras:

```text
BASH_VERSION
BASH_VERSINFO
BASHPID
PPID
BASH_SUBSHELL
BASH_ARGC
BASH_ARGV
BASH_ARGV0
BASH_COMMAND
BASH_SOURCE
BASH_LINENO
FUNCNAME
PIPESTATUS
BASH_ALIASES
BASH_CMDS
SHELLOPTS
BASHOPTS
RANDOM
SRANDOM
SECONDS
EPOCHSECONDS
EPOCHREALTIME
BASH_MONOSECONDS
```

## Opciones `set -o`

Reconocidas actualmente:

```text
allexport
braceexpand
emacs
errexit
errtrace
functrace
hashall
histexpand
history
ignoreeof
interactive-comments
keyword
monitor
noclobber
noexec
noglob
nolog
notify
nounset
onecmd
physical
pipefail
posix
privileged
verbose
vi
xtrace
```

## Opciones `shopt`

Reconocidas actualmente:

```text
array_expand_once
assoc_expand_once
autocd
bash_source_fullpath
cdable_vars
cdspell
checkhash
checkjobs
checkwinsize
cmdhist
compat31
compat32
compat40
compat41
compat42
compat43
compat44
compat50
compat51
compat52
compat53
complete_fullquote
direxpand
dirspell
dotglob
execfail
expand_aliases
extdebug
extglob
extquote
failglob
force_fignore
globasciiranges
globskipdots
globstar
gnu_errfmt
histappend
histreedit
histverify
hostcomplete
huponexit
inherit_errexit
interactive_comments
lastpipe
lithist
localvar_inherit
localvar_unset
login_shell
mailwarn
no_empty_cmd_completion
nocaseglob
nocasematch
noexpand_translation
nullglob
patsub_replacement
progcomp
progcomp_alias
promptvars
restricted_shell
shift_verbose
sourcepath
varredir_close
xpg_echo
```

## Estado real de compatibilidad Bash 5.3

La cobertura es amplia, pero **todavía no debe declararse equivalencia total con GNU Bash 5.3**.

Pendientes o diferencias conocidas:

| Área | Estado |
|---|---|
| `enable -f` / `enable -d` | No hay sistema de builtins cargables dinámicamente. |
| GNU Readline completo | SST implementa edición, history, completion y bindings propios, pero no replica toda GNU Readline 8.3. |
| `/dev/tcp/HOST/PORT` | No implementado actualmente. |
| `/dev/udp/HOST/PORT` | No implementado actualmente. |
| `test -u`, `test -g`, `test -k` | No tienen equivalente directo en NTFS y actualmente devuelven falso. |
| `test -O`, `test -G` | Aproximados; Windows no usa el modelo POSIX de propietario/grupo. |
| `test -x` | Aproximado al modelo de archivos de Windows. |
| `umask` | SST mantiene el valor lógico, pero Windows no aplica permisos POSIX al crear archivos. |
| `ulimit` | Mantiene los límites dentro del estado de la shell; no impone límites POSIX al proceso Windows. |
| `suspend` | Emulación interactiva, no suspensión POSIX real de la shell. |
| señales | Traducidas/adaptadas a procesos Windows; no existe equivalencia completa con las señales Unix. |
| `disown -h` | No reproduce literalmente la semántica SIGHUP de Unix. |
| controlling TTY | Windows/ConPTY no ofrece el mismo modelo de controlling terminal POSIX. |
| conformidad exhaustiva | Existe una suite comparativa básica, pero todavía no se ha certificado toda la semántica contra GNU Bash 5.3. |

---

# Terminal nativa

La interfaz gráfica de SST es una terminal Win32 propia e incluye:

- barra de título personalizada;
- controles propios de minimizar, maximizar y cerrar;
- backdrop/transparencia en Windows compatible;
- renderer VT;
- scrollback;
- selección con mouse;
- Nerd Font privada embebida;
- historial;
- autocompletado;
- soporte para TUIs;
- clipboard.

Atajos relevantes:

- `Ctrl+C`: copia cuando existe una selección; sin selección conserva el uso de interrupción.
- `Ctrl+V` / `Ctrl+Shift+V` / `Shift+Insert`: pegar.
- `Ctrl+Insert`: copiar.

---

# Datos portables

SST crea junto al ejecutable:

```text
config/
data/
```

Entre los datos persistentes se encuentran:

- `config/sstrc`: configuración de la shell;
- inventario de dispositivos;
- historial de presencia;
- perfiles de proveedores de red;
- perfiles de switches;
- historial de comandos;
- configuración y log de helix-sst.

---

# Estado general del proyecto

### Funcional

- terminal Win32 propia;
- intérprete Bash nativo con cobertura amplia de Bash 5.3;
- ejecución de scripts `.sh`;
- utilidades Unix incluidas;
- información y auditoría local de Windows;
- diagnóstico de red;
- escaneo y monitor de presencia;
- inventario de dispositivos;
- Wake-on-LAN;
- dominio;
- localización MAC → switch → puerto mediante SNMP;
- tráfico por proceso mediante ETW;
- configuración portable.

### Pendiente o parcial

- drivers reales de `net usage` para routers/AP/firewalls;
- equivalencia completa con GNU Readline;
- builtins dinámicos de Bash;
- `/dev/tcp` y `/dev/udp`;
- semántica Unix que no tiene equivalente directo en Windows;
- suite exhaustiva de conformidad Bash.

El documento de alcance y arquitectura está en:

```text
SHELL_SHOCK_TOOL_PLAN.md
```
