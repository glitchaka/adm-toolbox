# Shell Shock Tool

**Shell Shock Tool** es el nombre visual de `adm-tool` / `adm-toolbox`: una consola portable de administración y soporte técnico para Windows, construida en Rust.

Integra utilidades de red, diagnóstico, inventario, Wake-on-LAN, monitoreo de tráfico, consulta de dominio, localización de puertos de switch y herramientas de línea de comandos de uso cotidiano.

## Construcción

```powershell
cargo build --release
```

Ejecutable:

```text
target\release\adm-toolbox.exe
```

La aplicación crea su configuración portable en:

```text
config\admrc
```

junto al ejecutable. Puede editarse desde la propia shell:

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

- aliases;
- variables y export;
- funciones;
- sustitución de comandos;
- expansión de parámetros;
- redirecciones;
- pipes;
- operadores lógicos;
- globbing;
- scripts Bash/POSIX compatibles dentro de las capacidades del motor.

Las rutas del prompt se presentan al estilo Unix:

```text
C:\Users\manuel       -> ~
C:\Windows\System32   -> /c/Windows/System32
```

Y `cd /c/...` se traduce a la ruta Windows correspondiente.

## Utilidades Unix integradas

Además de los builtins Bash del motor, ADM Toolbox implementa/utiliza comandos familiares:

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

Actualmente correlaciona procesos, PID, ejecutable, CPU, RAM y conexiones/locales/remotas.

La medición exacta de bytes por segundo por PID está separada de esta primera capa y será implementada mediante telemetría ETW; ADM Toolbox no usa contadores de I/O genéricos como si fueran tráfico de red.

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

## Editor modal en Rust

```bash
vim archivo.conf
edit archivo.conf
```

El editor está implementado dentro de ADM Toolbox; no ejecuta `vim.exe`.

Funciones actuales:

- Normal, Insert, Command, Search y Visual Line;
- `h j k l`, flechas, `w`, `b`, `0`, `$`, `gg`, `G`;
- `i`, `a`, `o`, `O`;
- `x`, `dd`, `yy`, `p`;
- `u`, `Ctrl-R`;
- `V` para selección por líneas;
- `/texto`, `n`, `N`;
- `:w`, `:q`, `:q!`, `:wq`, `:x`;
- `:set number`, `:set nonumber`;
- `:%s/antiguo/nuevo/g`.

## Arquitectura

```text
Shell Shock Tool (adm-tool)
│
├── intérprete Bash propio en Rust
├── terminal nativa Win32
│   ├── renderer VT100
│   ├── Nerd Font embebida
│   ├── backdrop/transparencia
│   └── branding Shell Shock Tool
├── builtins ADM escritos en Rust
├── utilidades Unix integradas
└── editor modal tipo Vim escrito en Rust
```

El alcance completo está en `ADM_TOOLBOX_PLAN.md`.
