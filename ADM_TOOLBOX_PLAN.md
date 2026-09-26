# ADM Toolbox — Alcance y temas a cubrir

ADM Toolbox será una consola portable de soporte técnico para Windows, con experiencia cercana a Bash/Linux, pero capaz de utilizar tanto utilidades Unix incluidas en el paquete como herramientas y capacidades nativas de Windows.

El foco es administración y diagnóstico de equipos y redes autorizadas. La consola será el centro del producto; la GUI, si existe, será complementaria.

## Principio rector: primero una consola Linux, después las herramientas

ADM Toolbox no debe sentirse como una colección de utilidades pegadas alrededor de una consola. Debe sentirse, desde que abre hasta que cierra, como trabajar en una shell Linux/Bash coherente.

Esto implica:

- el prompt y la navegación son siempre el centro de la experiencia;
- no habrá un dashboard principal que sustituya a la terminal;
- no habrá botones, paneles o ventanas obligatorias para usar las funciones de soporte;
- todas las funciones importantes deben poder invocarse como comandos;
- los comandos propios deben comportarse como utilidades Unix: entrada simple, salida predecible, códigos de retorno y posibilidad de encadenarse;
- cualquier vista más rica será TUI opcional iniciada desde la propia shell, nunca una aplicación separada que rompa el flujo;
- los comandos deben agruparse por una taxonomía coherente y no crecer como nombres inconexos;
- `--help`, autocompletado y documentación deben hacer que descubrir funciones se sienta como usar herramientas Linux reales.

Ejemplo de sesión objetivo:

```bash
manuel@soporte ~
$ net scan --alive
10.10.20.15   00:11:22:33:44:55   LAB-PC-01
10.10.20.37   A4:C3:F0:11:93:02   NOTEBOOK-ALU

manuel@soporte ~
$ net locate 10.10.20.37
switch: SW-PISO2
port:   Gi1/0/27
vlan:   20

manuel@soporte ~
$ domain status 10.10.20.37
hostname: NOTEBOOK-ALU
domain:   WORKGROUP
joined:   no

manuel@soporte ~
$ wol LAB-PC-01
magic packet sent to 00:11:22:33:44:55
```

La experiencia debe ser coherente incluso al ejecutar programas nativos:

```bash
ipconfig /all | grep -i dns
tasklist | grep -i chrome
netstat -ano | grep LISTENING
```

---



## 1. Objetivos

- Ejecutable/carpeta portable para Windows.
- No depender de `cmd.exe` como interfaz principal.
- Experiencia de terminal tipo Bash/Linux.
- Scripts, pipes, redirecciones y aliases.
- Utilidades Unix incluidas.
- Acceso transparente a herramientas nativas de Windows.
- Herramientas propias para soporte técnico.
- Inventario y diagnóstico de red.
- Wake-on-LAN.
- Detección de equipos conectados.
- Identificación de pertenencia a dominio.
- Identificación, cuando la infraestructura lo permita, de la boca/puerto físico del switch donde está conectado un equipo.

---

## 2. Consola y shell

### 2.1 Terminal

- historial de comandos;
- navegación por historial;
- edición de línea;
- autocompletado;
- copiar/pegar;
- scrollback;
- UTF-8;
- colores ANSI;
- resize;
- prompt configurable;
- directorio actual visible;
- código de salida del último comando;
- indicador de privilegios elevados;
- pestañas o varias sesiones en una fase posterior.

### 2.2 Comportamiento tipo Bash

La shell base debe conservar la semántica y sensación de una terminal Linux. Los comandos ADM no deben introducir un sistema de interacción paralelo.

Debe soportar, directamente o mediante runtime integrado:

- `cd`
- `pwd`
- `ls`
- `cat`
- `less`
- `head`
- `tail`
- `grep`
- `sed`
- `awk`
- `find`
- `sort`
- `uniq`
- `cut`
- `xargs`
- `wc`
- `diff`
- `tee`
- `tar`
- `gzip`
- `zip/unzip`
- `curl`
- `ssh/scp/sftp`
- aliases;
- variables de entorno;
- pipes;
- redirecciones;
- scripts `.sh`;
- `&&`, `||`, `;`.

Ejemplo:

```bash
ipconfig /all | grep -i dns
tasklist | grep -i chrome
net scan --alive | sort
```

### 2.3 Convención de comandos ADM

Para evitar que el proyecto termine convertido en un pegote de comandos independientes, las capacidades propias se organizan por familias, igual que una buena CLI Unix moderna.

Estructura propuesta:

```text
net      red y descubrimiento
domain   dominio / Active Directory
switch   switches y puertos
device   inventario de equipos
wol      Wake-on-LAN
diag     diagnóstico
sys      información local
```

Ejemplos:

```bash
net scan
net scan 10.10.20.0/24
net neighbors
net ports 10.10.20.15 22,80,443

domain status LAB-PC-01
domain status 10.10.20.37

switch locate 00:11:22:33:44:55
switch show SW-PISO2

device list
device add 00:11:22:33:44:55 LAB-PC-01
device unknown

diag network
diag dns
diag storage

sys info
sys disks
sys services
```

Reglas de diseño:

- verbo y sustantivo consistentes;
- salida legible en terminal por defecto;
- `--json`, `--csv` o salida simple cuando corresponda;
- posibilidad de usar pipes;
- códigos de salida útiles para scripts;
- `--quiet` para scripts;
- `--help` en cada comando y subcomando;
- alias cortos solo cuando sean naturales;
- no duplicar comandos nativos de Windows o Unix sin una razón clara.

Ejemplo:

```bash
net scan --unknown --json | jq '.[] | .hostname'
```

### 2.4 TUI opcional, no GUI obligatoria

Cuando una operación se beneficie de una vista interactiva, podrá abrirse una TUI dentro de la terminal, por ejemplo:

```bash
net monitor
device tui
switch map
```

Estas vistas deben:

- ejecutarse dentro de la terminal;
- cerrarse y devolver al prompt;
- respetar teclado;
- no ser necesarias para acceder a ninguna función;
- no cambiar el modelo mental de Bash.

---



## 3. Integración con Windows

La shell debe poder ejecutar programas nativos del sistema:

- `ipconfig.exe`
- `ping.exe`
- `tracert.exe`
- `nslookup.exe`
- `arp.exe`
- `route.exe`
- `netstat.exe`
- `netsh.exe`
- `tasklist.exe`
- `taskkill.exe`
- `systeminfo.exe`
- `whoami.exe`
- `hostname.exe`
- `sc.exe`
- `reg.exe`
- `wevtutil.exe`
- `pnputil.exe`
- `driverquery.exe`
- `net.exe`
- `schtasks.exe`
- `robocopy.exe`
- PowerShell cuando esté disponible.

Conversión cómoda entre rutas:

```text
C:\Users\usuario\Desktop
/c/Users/usuario/Desktop
```

---

## 4. Comandos propios

### 4.1 Información local

```bash
sys info
sys processes
sys services
sys disks
sys memory
sys users
sys drivers
sys events
sys uptime
sys hostname

net interfaces
net routes
net connections
```

### 4.2 Diagnóstico

```bash
diag
diag network
diag dns
diag hardware
diag storage
```

La salida debe poder mostrarse en consola y exportarse a TXT, CSV o JSON.

---

## 5. Escaneo de red e inventario

El objetivo no es inspeccionar contenido de equipos ajenos, sino saber qué dispositivos están presentes en una red administrada y registrar datos operativos útiles.

### 5.1 Descubrimiento

Comandos previstos:

```bash
net scan
net scan 192.168.1.0/24
net scan --alive
net scan --details
net scan --unknown
```

Datos por equipo, cuando estén disponibles:

- IP;
- MAC;
- hostname;
- estado online/offline;
- latencia;
- interfaz local usada;
- fabricante de la MAC/OUI;
- fecha/hora de última detección;
- método con el que fue detectado.

### 5.2 Métodos

- ICMP;
- ARP;
- resolución DNS;
- vecinos IPv6;
- probes TCP controlados;
- datos del propio Windows;
- cachés y tablas locales.

---

## 6. Dominio / Active Directory

ADM Toolbox debe indicar si un equipo pertenece o no a un dominio y, cuando sea posible verificarlo, a cuál.

Salida deseada:

```text
IP              HOSTNAME        DOMINIO             ESTADO
10.10.20.15     LAB-PC-01       colegio.local       Dominio
10.10.20.22     DESKTOP-X91     WORKGROUP           No unido
10.10.20.37     NOTEBOOK-ALU    desconocido         No verificado
```

### 6.1 Estados

Distinguir explícitamente:

- unido a dominio;
- no unido / workgroup;
- dominio conocido;
- dominio no verificable;
- dato inferido;
- dato confirmado.

### 6.2 Fuentes posibles

Según permisos e infraestructura:

- DNS;
- Active Directory;
- LDAP;
- consultas WMI/CIM autorizadas;
- SMB/RPC cuando corresponda;
- nombre de dominio reportado por el propio host;
- inventario institucional existente.

No presentar una inferencia como confirmación.

---

## 7. Identificación de boca/puerto de switch

Requisito: poder saber, cuando la red lo permita, en qué puerto físico del switch aparece un equipo.

Ejemplo:

```text
HOST            IP            MAC                SWITCH        PUERTO
LAB-PC-01       10.10.20.15   00:11:22:33:44:55 SW-PISO2      Gi1/0/18
NOTEBOOK-ALU    10.10.20.37   A4:C3:F0:11:93:02 SW-PISO2      Gi1/0/27
```

### 7.1 Importante

Un escaneo IP por sí solo no puede saber la boca física del switch.

Para obtenerla hay que consultar la infraestructura de red mediante una o más de estas fuentes:

- tabla MAC/FDB del switch;
- SNMP;
- LLDP;
- CDP, si existe;
- API del fabricante/controlador;
- CLI remota autorizada;
- controlador central de red;
- inventario de switches.

### 7.2 Flujo de resolución

```text
IP
 ↓
MAC
 ↓
tabla MAC/FDB
 ↓
switch
 ↓
puerto físico
```

Si existen switches encadenados, el sistema debe seguir la MAC hasta encontrar el puerto de acceso final y distinguir enlaces trunk/uplink de puertos de usuario.

### 7.3 Datos a mostrar

- nombre/IP de switch;
- puerto;
- VLAN;
- descripción del puerto;
- estado del puerto;
- velocidad;
- trunk/access;
- PoE si aplica;
- última observación.

---

## 8. Equipos autorizados / desconocidos

Mantener una base local o importable de dispositivos institucionales.

Comandos previstos:

```bash
device add <MAC> <nombre>
device remove <MAC>
device list
net scan --unknown
net scan --authorized
```

Objetivo:

- detectar equipos no registrados;
- comparar contra inventario institucional;
- no etiquetar automáticamente como "intruso" algo que simplemente no esté inventariado.

Estados sugeridos:

- autorizado;
- conocido;
- desconocido;
- pendiente de revisión.

---

## 9. Monitorización de presencia

Modo periódico:

```bash
net monitor
net monitor --unknown
```

Eventos:

```text
[12:41:03] + Nuevo dispositivo
IP:       10.10.20.37
MAC:      A4:C3:F0:11:93:02
Hostname: NOTEBOOK-ALU
Dominio:  No unido
Switch:   SW-PISO2
Puerto:   Gi1/0/27
Estado:   Desconocido
```

Debe registrar:

- primera detección;
- última detección;
- cambios de IP;
- cambios de puerto;
- aparición/desaparición.

---

## 10. Wake-on-LAN

Comandos:

```bash
wol AA:BB:CC:DD:EE:FF
wol AA:BB:CC:DD:EE:FF 192.168.1.255
wol LAB-PC-01
```

Características:

- magic packet;
- broadcast automático o explícito;
- resolución por nombre desde inventario;
- posibilidad de enviar a varios equipos;
- grupos en una fase posterior.

---

## 11. Red y conectividad

Comandos propios o aliases para:

```bash
net ping
net trace
net dns
net arp
net neighbors
net ports
net interfaces
net routes
net connections
```

Ejemplos:

```bash
net ports 10.10.20.15 22,80,443,3389
net dns LAB-PC-01
net neighbors
```

El escaneo de puertos debe ser acotado y orientado a diagnóstico, no un escáner agresivo por defecto.

---

## 12. Tráfico local y consumo por proceso

ADM Toolbox debe incluir una herramienta equivalente, en espíritu, a combinar `nethogs`, `iftop`, `ss` y `top`, pero adaptada a Windows y manteniendo el flujo de una consola Linux.

Comando base:

```bash
net traffic
```

Debe mostrar qué procesos del equipo local están usando la red y cuánto consumen.

### 12.1 Vista por proceso

Salida esperada:

```text
PID    PROCESO        SUBIDA      BAJADA      CPU    RAM      DESTINOS
4120   chrome.exe     42 KB/s     310 KB/s    3.8%   684 MB   8
1884   OneDrive.exe   1.2 MB/s    95 KB/s     5.1%   221 MB   4
7316   updater.exe    180 KB/s    12 KB/s     9.4%   96 MB    2
```

Datos por proceso, cuando estén disponibles:

- PID;
- nombre del proceso;
- ruta completa del ejecutable;
- usuario que lo ejecuta;
- proceso padre;
- bytes enviados;
- bytes recibidos;
- velocidad actual de subida y bajada;
- total transferido durante la sesión;
- CPU;
- RAM;
- número de conexiones;
- IP y puerto local;
- IP y puerto remoto;
- protocolo;
- hostname remoto cuando pueda resolverse;
- estado de la conexión;
- firma digital del ejecutable cuando Windows pueda verificarla.

### 12.2 Modos de uso

```bash
net traffic
net traffic --watch
net traffic --top 20
net traffic --process chrome.exe
net traffic --pid 4120
net traffic --background
net traffic --external
net traffic --connections
net traffic --json
net traffic --csv
```

`net traffic --watch` debe comportarse como una herramienta Linux interactiva dentro de la terminal: actualización periódica, ordenación por uso y salida al prompt al cerrarla.

### 12.3 Detección de actividad anómala

La herramienta debe ayudar a detectar procesos que consumen recursos o transmiten datos sin que el usuario los esté utilizando activamente.

Puede señalar hechos observables como:

- tráfico sostenido en segundo plano;
- proceso con alto uso de red;
- CPU o RAM elevadas junto con actividad de red;
- conexiones a muchos destinos;
- ejecutable sin firma verificable;
- ejecutable corriendo desde una ruta inusual;
- proceso desconocido para el inventario local;
- transferencia continua durante largos periodos.

No debe afirmar automáticamente que un proceso es malware solo por presentar alguno de esos indicadores.

Comandos previstos:

```bash
net traffic --background
net traffic --unsigned
net traffic --high-usage
diag traffic
```

Ejemplo:

```text
PROCESO       RED            CPU    RAM     OBSERVACIONES
updater.exe   2.4 MB/s ↑     11%    96 MB   sin firma; actividad sostenida
Teams.exe     18 KB/s ↑      2%     410 MB  tráfico en segundo plano
svchost.exe   3 KB/s ↓       1%     54 MB   firmado por Microsoft
```

### 12.4 Destinos y privacidad

Para investigar telemetría o aplicaciones que envían información en segundo plano:

```bash
net traffic --process updater.exe --connections
```

Salida esperada:

```text
REMOTE                  PORT   PROTO   SENT       RECEIVED
telemetry.example.com   443    TCP     84.2 MB    1.8 MB
203.0.113.44            443    TCP     12.6 MB    620 KB
```

Por defecto se analizarán metadatos de conexión y volumen, no el contenido de los paquetes.

Una captura de paquetes completa será una capacidad separada y opcional si posteriormente se decide incorporar un proveedor como Npcap.

### 12.5 Implementación en Windows

La primera implementación debe evitar depender de drivers externos para las funciones básicas.

Fuentes previstas:

- ETW para atribución de tráfico por proceso;
- IP Helper API para conexiones TCP/UDP;
- APIs de procesos de Windows para PID, ruta, usuario, CPU y memoria;
- verificación Authenticode para firma digital;
- resolución DNS para destinos;
- contadores de rendimiento cuando aporten datos complementarios.

Cuando una métrica requiera privilegios elevados, la salida debe indicarlo claramente en vez de ocultar el dato o inventarlo.

### 12.6 Integración Unix

La salida debe poder encadenarse:

```bash
net traffic --json | jq '.[] | select(.upload_bps > 100000)'
net traffic --csv > trafico.csv
net traffic --background | grep -i unsigned
```

La TUI es opcional; la salida textual y procesable por pipes es obligatoria.

### 12.7 Uso de ancho de banda de toda la LAN

ADM Toolbox debe poder mostrar, desde la propia consola, qué dispositivos de la red están consumiendo el enlace a Internet.

Comando base:

```bash
net usage
```

Modos previstos:

```bash
net usage
net usage --watch
net usage --top
net usage --device 192.168.1.34
net usage --mac A4:C3:F0:11:93:02
net usage --json
net usage --csv
```

Salida esperada:

```text
DEVICE          IP              DOWN        UP          TOTAL
Notebook-Juan   192.168.1.34    81.7 Mbps   4.2 Mbps    85.9 Mbps
TV-Living       192.168.1.18    14.3 Mbps   212 Kbps    14.5 Mbps
PC-Manuel       192.168.1.10     1.8 Mbps    83 Kbps     1.9 Mbps
Telefono        192.168.1.25    320 Kbps     44 Kbps    364 Kbps
```

`net usage --watch` debe sentirse como `iftop` o `nethogs`, pero para toda la LAN.

#### Fuente de los datos

Un PC cliente no puede medir de forma fiable el tráfico total de todos los demás equipos de una red conmutada/Wi-Fi únicamente observando su propia interfaz.

Por tanto, `net usage` debe usar uno de estos proveedores:

- API del router/firewall;
- SNMP;
- controlador Wi-Fi;
- estadísticas del gateway;
- OpenWrt/OPNsense/pfSense u otro proveedor compatible;
- port mirroring/SPAN cuando exista una interfaz de captura autorizada.

El comando debe abstraer el proveedor. La experiencia de uso seguirá siendo siempre:

```bash
net usage --watch
```

sin obligar al técnico a entrar a la GUI del router.

Configuración prevista:

```bash
net provider add home-router --type snmp --host 192.168.1.1
net provider add firewall --type opnsense --host 10.0.0.1
net provider list
net provider use home-router
```

Las credenciales se almacenarán mediante el mecanismo seguro definido para proveedores y nunca en texto plano dentro de scripts.

Si el router no expone estadísticas por cliente, el comando debe indicarlo explícitamente y no inventar consumo por dispositivo.

---

## 13. Switches y credenciales

La herramienta debe soportar perfiles de infraestructura sin incrustar contraseñas en scripts.

Temas a cubrir:

- credenciales almacenadas de forma segura;
- perfiles por fabricante;
- SNMPv2/SNMPv3;
- SSH autorizado;
- API REST cuando exista;
- timeouts;
- reintentos;
- logs;
- separación entre lectura y acciones de cambio.

Inicialmente, la integración con switches debe ser de solo lectura.

---

## 14. Inventario

Posibilidad de mantener:

- equipos;
- MAC;
- hostname;
- IP;
- dominio;
- switch;
- puerto;
- VLAN;
- ubicación;
- propietario institucional;
- estado;
- notas.

Importación/exportación:

- CSV;
- JSON;
- TXT;
- posible integración posterior con inventarios existentes.

---

## 15. Seguridad operacional

ADM Toolbox debe:

- distinguir operaciones de lectura de operaciones destructivas;
- pedir confirmación para acciones peligrosas;
- mostrar claramente si está elevado;
- registrar qué comando se ejecutó;
- evitar guardar contraseñas en texto plano;
- no asumir autorización sobre redes externas;
- limitar por defecto los escaneos al rango explícito o a la subred local.

---

## 16. Lo que ADM Toolbox no debe convertirse en

- No debe convertirse en un dashboard de administración.
- No debe convertirse en una colección de ventanas.
- No debe esconder comandos detrás de botones.
- No debe tener una interfaz distinta para cada herramienta.
- No debe reemplazar Bash por un menú de opciones.
- No debe obligar a usar mouse.
- No debe introducir nombres arbitrarios cuando existe una convención Unix comprensible.
- No debe mezclar salida decorativa con salida pensada para scripts.
- No debe sacrificar pipes, redirecciones o automatización por una presentación visual.

La pregunta de diseño para cada nueva característica será:

> ¿Cómo se usaría esto si fuera una utilidad nativa de Linux instalada en `/usr/bin`?

Solo después de resolver su interfaz de línea de comandos se considerará una TUI o representación visual opcional.

---

## 17. Portabilidad

Estructura prevista:

```text
adm-toolbox/
├── adm-toolbox.exe
├── runtime/
│   ├── bash.exe
│   ├── grep.exe
│   ├── sed.exe
│   ├── awk.exe
│   └── ...
├── scripts/
├── data/
│   ├── devices.json
│   └── switches.json
├── logs/
└── home/
```

No requerir instalación tradicional para la consola base.

---

## 18. Arquitectura

Principios:

- orientación a objetos donde tenga sentido;
- SOLID;
- separación entre terminal, comandos, servicios y proveedores de red;
- comandos desacoplados;
- proveedores intercambiables para switches;
- capa de inventario separada;
- configuración externa;
- servicios testeables.

Posible organización:

```text
Terminal
  ↓
Command Dispatcher
  ├── Unix runtime
  ├── Windows command bridge
  └── ADM commands
        ├── NetworkScanner
        ├── DomainResolver
        ├── SwitchPortResolver
        ├── WakeOnLanService
        ├── TrafficMonitorService
        ├── ProcessResourceService
        ├── InventoryService
        └── DiagnosticService
```

---

## 19. Prioridad inicial

Primera versión funcional, manteniendo siempre la shell como producto principal:

1. terminal portable con experiencia Bash/Linux;
2. ejecución de comandos Windows sin abandonar la shell;
3. utilidades Unix básicas;
4. pipes, redirecciones, aliases y scripts;
5. autocompletado y `--help` coherentes;
6. familia `net`: descubrimiento y listado IP/MAC/hostname;
7. familia `domain`: estado y dominio de cada equipo;
8. `wol`;
9. familia `device`: inventario local y equipos desconocidos;
10. `net scan --unknown`;
11. `net traffic`: tráfico por proceso, destinos y consumo de CPU/RAM;
12. familia `switch`: resolución switch/puerto mediante proveedor configurable;
13. exportación CSV/JSON;
14. TUI opcionales solo donde realmente aporten valor.

Las capacidades de cambio de configuración de switches quedan fuera de la primera etapa.
