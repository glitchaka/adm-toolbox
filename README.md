# Shell Shock Tool

**Shell Shock Tool** es una consola portable de administración y soporte técnico para Windows, construida en Rust.

Integra utilidades de red, diagnóstico, inventario, Wake-on-LAN, monitoreo de tráfico, consulta de dominio, localización de puertos de switch y herramientas de línea de comandos de uso cotidiano.

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

La shell conserva estado entre comandos y entiende construcciones Bash reales, no solamente una lista de comandos que imitan Linux.

Ejemplos:

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

También conserva:

- aliases, variables, `export` y `readonly`;
- funciones con `local` y `return`;
- `if`, `for`, `for ((...))`, `select`, `while`, `until` y `case ... esac`;
- terminadores de `case` `;;`, `;&` y `;;&`;
- `break` y `continue`, incluidos niveles de bucle;
- condicionales `[[ ... ]]` con cadenas, enteros, archivos y expresiones regulares;
- comandos y expansiones aritméticas con asignaciones, incremento/decremento y precedencia de operadores;
- arrays indexados y asociativos, `declare`, `typeset`, `mapfile` y `readarray`;
- sustitución de comandos, expansión de parámetros, brace expansion, IFS y globbing;
- heredocs, here-strings y redirecciones de entrada/salida;
- pipes, operadores lógicos y opciones como `pipefail`, `errexit`, `nounset`, `noexec`, `xtrace`, `noclobber` y `allexport`;
- `read` interactivo con opciones de prompt, modo silencioso, modo raw, límites de caracteres y arrays;
- traps `ERR` y `EXIT`, ejecución en segundo plano, `jobs`, `wait` y `fg`;
- ejecución directa de scripts `.sh` y archivos con shebang Bash/sh;
- scripts Bash/POSIX compatibles dentro de las capacidades del motor.

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

## Estado actual

Implementado actualmente:

- shell Bash-compatible y utilidades Unix integradas;
- terminal Win32 propia, historial, autocompletado y configuración portable;
- información de sistema, procesos, discos, memoria y uptime;
- consultas administrativas de servicios, usuarios, drivers/PnP, Event Log, Registro y tareas programadas;
- diagnóstico y descubrimiento de red;
- inventario y presencia persistente de dispositivos;
- Wake-on-LAN;
- estado de dominio local/remoto;
- localización MAC → switch → puerto mediante SNMP de solo lectura;
- tráfico por proceso mediante ETW y detección del proceso foreground;
- editor portable `helix-sst`.

Pendiente dentro del alcance actual:

- proveedores reales de `net usage` para obtener consumo por dispositivo desde router/AP/firewall;
- verificación Authenticode;
- auditoría estructurada de comandos y acciones administrativas;
- indicador central de elevación/UAC;
- confirmación centralizada para operaciones destructivas;
- proveedores adicionales de infraestructura y compatibilidad Bash/Unix adicional cuando sea necesaria.

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
