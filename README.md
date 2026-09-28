# Shell Shock Tool

**Shell Shock Tool** es una consola portable de administración y soporte técnico para Windows, construida en Rust.

Integra utilidades de red, diagnóstico, inventario, Wake-on-LAN, monitoreo de tráfico, consulta de dominio, localización de puertos de switch y herramientas de línea de comandos de uso cotidiano.

## Release actual

El corte publicado actualmente es **Shell Shock Tool v0.1.4**, correspondiente al commit `70d0fc737b66156769319a096560bb177b266de1`.

Este release congela el avance actual de la capa Bash nativa. El workflow de publicación terminó correctamente.

La preparación de este corte se realizó **sin compilar ni ejecutar la suite de pruebas**.

## Construcción

```powershell
cargo build --release
```

El ejecutable se genera dentro de:

```text
target\release\
```

La aplicación mantiene una configuración portable local que puede editarse desde la propia shell:

```bash
config edit
config reload
```

## Interfaz gráfica y tipografía

La terminal nativa de Shell Shock Tool tiene interfaz propia Win32:

- barra de título personalizada con el icono/mascota;
- controles de minimizar, maximizar y cerrar inspirados en los tres botones de la mascota;
- fondo oscuro translúcido con backdrop DWM en Windows 11;
- prompt Nerd Font;
- selección con mouse y scrollback;
- `Ctrl+C` copia cuando hay selección y conserva la interrupción cuando no la hay;
- `Ctrl+V`, `Ctrl+Shift+V` y `Shift+Insert` pegan texto;
- `Ctrl+Insert` copia.

La terminal lleva **JetBrainsMono Nerd Font Mono embebida dentro del ejecutable**. Se carga de forma privada en memoria al iniciar y se libera al cerrar, por lo que no requiere instalar fuentes en el PC y funciona desde un pendrive.

Durante la compilación, `build.rs` obtiene la fuente oficial de Nerd Fonts y la incorpora al binario. Para compilar sin Internet puede definirse `ADM_NERD_FONT_FILE` apuntando a una copia local compatible.

El icono de Shell Shock Tool se genera e incrusta como recurso de Windows a partir del diseño de la mascota. El SVG fuente está en `assets/shell-shock-mascot.svg`.

## Shell Bash-compatible

Shell Shock Tool incluye un intérprete Bash propio escrito en Rust. La versión `v0.1.4` amplía de forma importante la cobertura de Bash 5.3, pero no pretende afirmar compatibilidad binaria ni semántica total con GNU Bash sobre Unix.

Ejemplos básicos:

```bash
name="laboratorio"
echo "$name"

for host in 1 2 3; do
    echo "192.168.1.$host"
done

scan_lab() {
    net scan 192.168.1.0/24 --unknown
}

scan_lab

cat archivo.txt | grep -i error | sort
net scan --unknown --json > desconocidos.json
command_that_works && echo ok
command_that_fails || echo fallo
```

### Cobertura implementada en v0.1.4

El motor actual incluye, entre otras capacidades:

- variables, parámetros posicionales y variables especiales de Bash;
- aliases y funciones;
- scopes locales, `local`, `return`, `break`, `continue` y atributos de variables;
- arrays indexados, arrays asociativos, namerefs y arrays dispersos;
- `declare`, `typeset`, `readonly`, `mapfile` y `readarray`;
- `if`, `for`, `for ((...))`, `select`, `while`, `until` y `case ... esac`;
- grupos `{ ...; }`, subshells `(...)`, `[[ ... ]]` y `(( ... ))`;
- sustitución de comandos, expansiones aritméticas y de parámetros;
- brace expansion, tilde expansion, IFS y quoting ANSI-C/localizado;
- globbing, `extglob`, `globstar`, `GLOBIGNORE`, `GLOBSORT`, `dotglob`, `nullglob`, `failglob` y opciones relacionadas;
- heredocs, here-strings, duplicación de descriptores, redirecciones combinadas y descriptores asignados a variables;
- pipes, `|&`, pipelines paralelos, `pipefail`, operadores `&&`, `||`, `!` y background con `&`;
- coprocesos y process substitution `<(...) / >(...)`;
- jobs con `jobs`, `fg`, `bg`, `wait`, `wait -n` y `disown`;
- traps, incluidos `ERR`, `EXIT`, `DEBUG` y `RETURN` dentro de las capacidades de Windows;
- historial, `history`, `fc`, `bind` y programmable completion con `complete`, `compgen` y `compopt`;
- `set`, `shopt`, opciones de compatibilidad y `compat53`;
- builtins como `hash`, `getopts`, `exec`, `enable`, `suspend`, `dirs`, `pushd`, `popd`, `umask`, `ulimit`, `times` y `caller`;
- variables y estado especiales como `PIPESTATUS`, `BASHPID`, `BASH_SUBSHELL`, `BASH_ARGC`, `BASH_ARGV`, `FUNCNAME`, `BASH_SOURCE`, `BASH_LINENO`, `RANDOM`, `SRANDOM`, `SECONDS`, `EPOCHSECONDS`, `EPOCHREALTIME`, `BASH_MONOSECONDS`, `BASH_ALIASES` y `BASH_CMDS`;
- cambios específicos de Bash 5.3 ya incorporados en el motor, como `read -E`, `compgen -V`, `source -p`, `trap -P`, `array_expand_once`, `bash_source_fullpath` y sustituciones ejecutadas en el shell actual.

### Scripts

Shell Shock Tool reconoce archivos `.sh` y archivos con shebang Bash/sh, además de aceptar un script como argumento del ejecutable.

```bash
test.sh
./test.sh
adm-toolbox.exe test.sh
```

**Limitación conocida de v0.1.4:** los scripts lanzados desde la shell todavía se despachan mediante una segunda instancia de `adm-toolbox.exe`. En la terminal Win32 propia, un script que necesite entrada interactiva mediante `read` puede fallar al heredar `stdin` con `Controlador no válido (os error 6)`. El reconocimiento y despacho de scripts está implementado; el puente de entrada interactiva de ese proceso hijo sigue pendiente de corrección.

### Diferencias deliberadas o pendientes frente a GNU Bash 5.3

- el control de jobs y las señales se adaptan a procesos y APIs de Windows; no existe un controlling TTY POSIX idéntico al de Unix;
- pruebas de archivo ligadas a permisos/propietario Unix, como setuid, setgid, sticky bit y ejecutabilidad POSIX, solo pueden aproximarse o carecen de equivalente directo;
- `disown -h` no reproduce literalmente el comportamiento de SIGHUP de Unix;
- los builtins cargables dinámicamente mediante `enable -f/-d` no están disponibles;
- la edición interactiva emula interfaces de Bash/Readline, pero no incorpora GNU Readline 8.3 completo;
- la conformidad amplia con GNU Bash 5.3 todavía no ha sido certificada mediante una suite exhaustiva comparativa.

Las rutas del prompt se presentan al estilo Unix:

```text
C:\Users\manuel       -> ~
C:\Windows\System32   -> /c/Windows/System32
```

Y `cd /c/...` se traduce a la ruta Windows correspondiente.

## Utilidades Unix integradas

Además de los builtins Bash del motor, Shell Shock Tool implementa/utiliza comandos familiares:

```bash
ls
cat
head
tail
grep
wc
sort
uniq
cut
tee
less
more
sed
awk
diff
find
basename
dirname
realpath
date
sleep
sha256sum
base64
touch
mkdir
rm
cp
mv
which
```

También:

```bash
fetch
neofetch
fastfetch
ps
top
df
free
uptime
hostname
whoami
uname
kill
```

`fetch`, `neofetch` y `fastfetch` muestran la mascota de Shell Shock Tool como ASCII art junto con la información del sistema. Usa `fetch --small` para una variante compacta.

`top` y `less` funcionan como TUI dentro de la terminal y regresan al prompt al cerrarse.

La shell puede ejecutar directamente programas disponibles en Windows:

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

## Sistema y administración local

Las consultas administrativas habituales están disponibles como subcomandos propios de `sys`. Shell Shock Tool usa las utilidades nativas de Windows como backend, manteniendo una interfaz única dentro de la shell.

```bash
sys info
sys processes
sys top
sys disks
sys memory
sys uptime
sys hostname
sys whoami

sys services
sys services --running
sys services Spooler

sys users
sys users Administrador
sys users --domain

sys drivers
sys drivers --verbose
sys drivers --signed
sys drivers --pnp
sys drivers --devices

sys events
sys events Application --count 50
sys events --log Security --count 20
sys events --logs

sys registry "HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion"
sys registry "HKLM\\SOFTWARE\\Microsoft" --recursive
sys registry "HKCU\\Software" --find Shell

sys tasks
sys tasks --verbose
sys tasks "\\Microsoft\\Windows\\Defrag\\ScheduledDefrag"
```

Estas vistas son de consulta/diagnóstico. No modifican servicios, cuentas, drivers, Event Log, Registro ni tareas programadas. Las operaciones destructivas se mantendrán separadas hasta que exista una capa central de privilegios, confirmación y auditoría.

Los backends utilizados son `sc.exe`, `net.exe`, `driverquery.exe`, `pnputil.exe`, `wevtutil.exe`, `reg.exe` y `schtasks.exe`; por tanto siguen disponibles también directamente desde la shell cuando se necesiten opciones avanzadas.


## Red

```bash
net interfaces
net connections
net routes
net neighbors
net dns equipo
net ping equipo
net trace equipo
net scan
net scan 192.168.1.0/24
net scan --unknown
net scan --authorized
net scan --json
net scan --csv
net monitor
net monitor --unknown
net presence
net ports 192.168.1.25 22,80,443,3389
```

`net monitor` mantiene historial de presencia y registra aparición, desaparición y cambios de IP.

## Tráfico local

```bash
net traffic
net traffic --watch
net traffic --top 20
net traffic --process chrome
net traffic --pid 4120
net traffic --connections
net traffic --json
net traffic --csv
```

Actualmente correlaciona procesos, PID, ejecutable, CPU, RAM, conexiones locales/remotas y tráfico de red por proceso. La medición de subida y bajada por PID usa ETW de Windows y calcula tasas a partir de los eventos de red; si ETW no está disponible, la vista degrada de forma explícita sin fingir contadores de red.

La detección de proceso en primer plano también está implementada mediante `GetForegroundWindow` y `GetWindowThreadProcessId`, lo que permite distinguir el proceso activo en las vistas de tráfico.

## Uso de Internet de toda la LAN

```bash
net provider add home-router --type openwrt --host 192.168.1.1
net provider list
net provider use home-router
net provider current
net provider capabilities
net usage
```

`net usage` solo mostrará consumo por dispositivo cuando el router/AP/firewall configurado entregue esos contadores. No inventa tráfico a partir de ARP o ping.

## Inventario

```bash
device add AA:BB:CC:DD:EE:FF LAB-PC-01
device add AA:BB:CC:DD:EE:FF LAB-PC-01 --note "Sala 3"
device list
device list --json
device list --csv
device show LAB-PC-01
device remove AA:BB:CC:DD:EE:FF
```

El inventario se integra con:

```bash
net scan --unknown
net scan --authorized
wol LAB-PC-01
```

## Wake-on-LAN

```bash
wol AA:BB:CC:DD:EE:FF
wol LAB-PC-01
wol LAB-PC-01 192.168.1.255
```

## Dominio

```bash
domain status
domain status EQUIPO
domain status EQUIPO --verify
domain status EQUIPO --json
```

Para el equipo local intenta obtener una respuesta confirmada desde `Win32_ComputerSystem`. Para un host remoto, sin `--verify`, distingue explícitamente una inferencia DNS de una comprobación real.

## Switch / boca física

Perfiles de switch:

```bash
export SW_CORE_COMMUNITY='comunidad-snmp'

switch add SW-PISO2 --host 10.0.0.12 --community-env SW_CORE_COMMUNITY
switch list
switch show SW-PISO2
```

Localización:

```bash
switch locate AA:BB:CC:DD:EE:FF
switch locate LAB-PC-01
switch locate LAB-PC-01 --switch SW-PISO2
switch locate LAB-PC-01 --vlan 20
switch locate LAB-PC-01 --json
```

La primera integración es **solo lectura** mediante SNMPv2c y consulta Bridge-MIB/Q-BRIDGE-MIB/IF-MIB para resolver:

```text
MAC -> switch -> bridge port -> ifIndex -> interfaz
```

y, cuando el equipo lo expone, PVID, velocidad, alias y estado operativo.

La comunidad SNMP no se guarda en el inventario. El perfil guarda únicamente el nombre de una variable de entorno que contiene la credencial.

## Diagnóstico

```bash
diag network
diag dns
diag hardware
diag storage
diag traffic
diag domain
```

## Editor integrado: helix-sst

Shell Shock Tool integra `helix-sst 0.1.0`, basado en **Helix 25.07.1**.

```bash
helix
helix archivo.conf
helix script.sh
hx script.sh
helix --version
helix --credits
```

La distribución oficial de Helix 25.07.1 para Windows queda embebida durante la compilación y se despliega de forma portable al primer uso. Shell Shock Tool añade su propio tema, configuración portable, puente PTY e integración de comandos.

Proyecto original: `helix-editor/helix`  
Versión upstream integrada: `25.07.1`  
Licencia upstream: Mozilla Public License 2.0 (MPL-2.0)  
Créditos: Helix contributors.

Los avisos y la licencia correspondientes se conservan en `THIRD_PARTY_NOTICES.md` y `licenses/HELIX-MPL-2.0.txt`.

Para una compilación sin Internet puede definirse `ADM_HELIX_ARCHIVE` apuntando al ZIP oficial `helix-25.07.1-x86_64-windows.zip`.

## Estado actual de v0.1.4

Implementado actualmente:

- intérprete Bash propio en Rust con cobertura amplia de Bash 5.3;
- scripts `.sh` y shebang Bash/sh reconocidos y despachados por Shell Shock Tool;
- terminal Win32 propia, historial, autocompletado y configuración portable;
- jobs/background, pipelines paralelos, coprocesos y process substitution;
- arrays, namerefs, atributos, globbing avanzado, history, completion y variables especiales de Bash;
- información de sistema, procesos, discos, memoria y uptime;
- consultas administrativas de servicios, usuarios, drivers/PnP, Event Log, Registro y tareas programadas;
- diagnóstico y descubrimiento de red;
- inventario y presencia persistente de dispositivos;
- Wake-on-LAN;
- estado de dominio local/remoto;
- localización MAC → switch → puerto mediante SNMP de solo lectura;
- tráfico por proceso mediante ETW y detección del proceso foreground;
- editor portable `helix-sst`.

Pendiente o conocido en este corte:

- corregir el `stdin` interactivo de scripts `.sh` lanzados desde la terminal Win32;
- ampliar la suite de conformidad contra GNU Bash 5.3 antes de declarar compatibilidad completa;
- proveedores reales de `net usage` para obtener consumo por dispositivo desde router/AP/firewall;
- verificación Authenticode;
- auditoría estructurada de comandos y acciones administrativas;
- indicador central de elevación/UAC;
- confirmación centralizada para operaciones destructivas;
- proveedores adicionales de infraestructura;
- diferencias de plataforma inevitables o todavía no emuladas respecto de permisos, señales, TTY POSIX y GNU Readline completo.

## Arquitectura

```text
Shell Shock Tool
│
├── intérprete Bash propio en Rust
├── terminal nativa Win32
│   ├── renderer VT100
│   ├── Nerd Font embebida
│   ├── backdrop/transparencia
│   └── branding Shell Shock Tool
├── builtins administrativos escritos en Rust
├── utilidades Unix integradas
└── helix-sst 0.1.0 (basado en Helix 25.07.1)
```

El alcance completo está en `ADM_TOOLBOX_PLAN.md`.
