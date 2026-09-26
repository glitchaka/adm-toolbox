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
net net scan --unknown --json | jq '.[] | .hostname'
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
sysinfo
interfaces
routes
connections
processes
services
disks
memory
users
drivers
events
uptime
hostname
```

### 4.2 Diagnóstico

```bash
diagnose
diagnose network
diagnose dns
diagnose hardware
diagnose storage
```

La salida debe poder mostrarse en consola y exportarse a TXT, CSV o JSON.

---

## 5. Escaneo de red e inventario

El objetivo no es inspeccionar contenido de equipos ajenos, sino saber qué dispositivos están presentes en una red administrada y registrar datos operativos útiles.

### 5.1 Descubrimiento

Comandos previstos:

```bash
scan
scan 192.168.1.0/24
scan --alive
scan --details
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
watch-net
watch-net --unknown
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
ping
trace
dns
arp
neighbors
ports
interfaces
routes
connections
```

Ejemplos:

```bash
ports 10.10.20.15 22,80,443,3389
dns LAB-PC-01
neighbors
```

El escaneo de puertos debe ser acotado y orientado a diagnóstico, no un escáner agresivo por defecto.

---

## 12. Switches y credenciales

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

## 13. Inventario

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

## 14. Seguridad operacional

ADM Toolbox debe:

- distinguir operaciones de lectura de operaciones destructivas;
- pedir confirmación para acciones peligrosas;
- mostrar claramente si está elevado;
- registrar qué comando se ejecutó;
- evitar guardar contraseñas en texto plano;
- no asumir autorización sobre redes externas;
- limitar por defecto los escaneos al rango explícito o a la subred local.

---

## 15. Lo que ADM Toolbox no debe convertirse en

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

## 16. Portabilidad

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

## 17. Arquitectura

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
        ├── InventoryService
        └── DiagnosticService
```

---

## 18. Prioridad inicial

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
11. familia `switch`: resolución switch/puerto mediante proveedor configurable;
12. exportación CSV/JSON;
13. TUI opcionales solo donde realmente aporten valor.

Las capacidades de cambio de configuración de switches quedan fuera de la primera etapa.
