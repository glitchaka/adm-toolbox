# Shell Shock Tool — Pendientes reales

Este archivo contiene **únicamente trabajo pendiente de implementación o de cierre técnico** en el estado actual de `main`.

No es una descripción general de SST ni un registro histórico. Las capacidades ya implementadas se documentan en `README.md`.

Estado de referencia al actualizar este archivo:

- producto: **Shell Shock Tool (SST)**;
- versión: **0.1.5**;
- plataforma principal: **Windows**;
- implementación: **Rust**;
- shell: **intérprete Bash-compatible propio de SST**;
- objetivo de compatibilidad: **Bash 5.3**;
- no se usa `brush-core`/`brush-builtins` como motor de la shell;
- Xilem no forma parte de la base estable actual.

---

## 1. `net usage`: consumo de Internet de toda la LAN

La infraestructura de configuración de proveedores ya existe, pero todavía falta implementar la obtención real de contadores de tráfico por cliente.

Interfaz objetivo:

```bash
net usage
net usage --watch
net usage --top
net usage --device 192.168.1.34
net usage --mac AA:BB:CC:DD:EE:FF
net usage --json
net usage --csv
```

Debe mostrar, cuando el proveedor lo permita:

- dispositivo;
- IP;
- MAC;
- descarga actual;
- subida actual;
- tráfico total o acumulado disponible;
- origen/proveedor de la medición.

### Pendiente

Implementar drivers reales para uno o más de los proveedores ya contemplados por SST:

- OpenWrt;
- OPNsense;
- pfSense;
- UniFi;
- SNMP;
- proveedor genérico cuando exista una API configurable adecuada.

La arquitectura debe mantener `net usage` independiente del fabricante.

SST no debe inferir consumo por cliente mediante ARP, ping ni observación de la interfaz local. Si el gateway/controlador no expone contadores por dispositivo, debe indicarlo explícitamente.

### Credenciales

Cuando un proveedor necesite autenticación, falta cerrar un mecanismo seguro para sus credenciales. No deben persistirse secretos en texto plano dentro de scripts ni de la configuración portable.

---

## 2. Compatibilidad Bash 5.3 restante

El motor Bash de SST ya cubre la mayor parte del lenguaje, expansiones, redirecciones, arrays, funciones, traps, jobs, process substitution, coprocesos, programmable completion y builtins principales.

No debe sustituirse por Brush ni por un `bash.exe` externo.

### 2.1 Builtins cargables dinámicamente

Pendiente:

```bash
enable -f archivo builtin
enable -d builtin
```

Actualmente SST no dispone de un ABI/sistema para cargar y descargar builtins binarios dinámicamente.

La implementación deberá definir explícitamente cómo se adapta esta capacidad al modelo Rust/Windows sin comprometer la portabilidad o seguridad de SST.

### 2.2 `/dev/tcp` y `/dev/udp`

Pendiente implementar las redirecciones especiales compatibles con Bash:

```bash
/dev/tcp/HOST/PORT
/dev/udp/HOST/PORT
```

Deben integrarse con el sistema normal de redirecciones del intérprete y respetar códigos de error y descriptores.

### 2.3 GNU Readline 8.3

SST ya tiene edición de línea, historial, completion y bindings propios.

Falta ampliar la compatibilidad donde sea razonable con el comportamiento de GNU Readline 8.3, especialmente:

- comandos/bindings todavía no reproducidos;
- variables de Readline;
- modos y comportamiento fino de edición;
- semántica de completion dependiente de Readline.

No se requiere incorporar GNU Readline como dependencia si la semántica puede implementarse en el frontend propio.

---

## 3. Diferencias POSIX/Windows que requieren cierre o documentación definitiva

Estas áreas no son simples comandos ausentes: Bash depende aquí de primitivas POSIX que Windows no ofrece de forma equivalente.

### `test`

Pendiente decidir o mejorar la adaptación de:

```text
test -u
test -g
test -k
test -O
test -G
test -x
```

Estado actual:

- `-u`, `-g` y `-k` devuelven falso;
- `-O`, `-G` y `-x` son aproximaciones al modelo Windows.

No se debe fingir semántica POSIX inexistente.

### `umask`

SST mantiene actualmente un valor lógico, pero Windows/NTFS no aplica el modelo POSIX de creación de archivos.

Pendiente decidir si se implementa una traducción útil a ACL de Windows o se conserva explícitamente como compatibilidad lógica.

### `ulimit`

Los límites existen en el estado de la shell, pero no se imponen como límites POSIX reales al proceso.

Pendiente evaluar equivalentes mediante primitivas de Windows —por ejemplo Job Objects cuando corresponda— y documentar qué límites pueden ser efectivos.

### Señales, `suspend`, `disown -h` y controlling TTY

Pendiente cerrar la mejor equivalencia posible para:

- señales Unix sobre procesos Windows;
- `suspend`;
- semántica de `disown -h`;
- comportamiento dependiente de controlling TTY;
- interacción de job control con ConPTY.

La meta es compatibilidad observable donde Windows lo permita, no simular garantías POSIX que el sistema operativo no proporciona.

---

## 4. Conformidad Bash 5.3

Existe una suite comparativa básica, pero todavía falta una campaña exhaustiva de conformidad contra GNU Bash 5.3.

### Pendiente

Ampliar `scripts/conformance.ps1` para cubrir de forma sistemática:

- lexer y parser;
- quoting;
- expansiones de parámetros;
- expansión aritmética;
- arrays indexados y asociativos;
- namerefs;
- scopes;
- funciones;
- variables especiales;
- `set -o`;
- `shopt`;
- redirecciones;
- heredocs y here-strings;
- process substitution;
- coprocesos;
- pipelines;
- códigos de salida y `PIPESTATUS`;
- traps;
- job control;
- `read`, `mapfile` y entrada interactiva;
- completion;
- historial;
- ejecución de scripts;
- casos límite de Bash 5.3.

La referencia debe ser el comportamiento observable de GNU Bash 5.3. Las diferencias inevitables por Windows deben quedar clasificadas explícitamente como adaptaciones de plataforma y no como conformidad exacta.

---

## 5. Criterio para retirar elementos de este archivo

Una tarea debe eliminarse de este plan cuando:

1. la capacidad esté implementada en `main`;
2. la documentación pública correspondiente refleje su comportamiento real;
3. si requiere validación de compatibilidad, exista cobertura suficiente en la suite de conformidad;
4. cualquier diferencia inevitable con Linux/POSIX esté documentada de forma explícita.

Este archivo no debe volver a acumular funciones que SST ya tenga implementadas.
