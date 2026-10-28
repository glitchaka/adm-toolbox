# Arch Linux packages — candidatos de portabilidad para SST

Este documento es el índice de trabajo para revisar paquetes de los repositorios oficiales de Arch Linux y decidir si pueden convertirse en herramientas nativas de Windows/SST.

No significa «ejecutar paquetes de Arch en Windows» ni usar WSL. El objetivo es localizar proyectos útiles en Arch, identificar su upstream y, cuando sea viable, usar un build Windows oficial o portar/adaptar el código para producir una herramienta Windows real que pueda distribuir `instp`.

## Catálogo completo y vivo

Fuente oficial de todos los paquetes:

- https://archlinux.org/packages/
- API oficial de búsqueda: https://archlinux.org/packages/search/json/
- API moderna: https://archlinux.org/api/v1/packages/search/

Arch cambia continuamente. Por eso este documento no copia miles de nombres que quedarían obsoletos: la lista completa se consulta en la fuente oficial y aquí se mantiene el estado de portabilidad de los paquetes que SST vaya revisando.

Snapshot de referencia al crear este documento (2026-10-28): 15.849 entradas visibles en los repositorios oficiales de Arch.

## Estados SST

| Estado | Significado | Acción |
| --- | --- | --- |
| `DIRECTO` | El upstream ya publica un binario Windows/Win64 utilizable por consola. | Añadir manifiesto a `packages/index.json` o repositorio `instp`. |
| `ADAPTABLE` | No hay build Windows listo, pero el código puede portarse razonablemente sustituyendo dependencias POSIX/Linux. | Crear fork/patch Windows y publicar releases para `instp`. |
| `REVISAR` | No basta con nombre/descripción; hay que revisar upstream, licencia, lenguaje y dependencias. | No integrar hasta revisar código. |
| `LINUX-ONLY` | Depende estructuralmente de kernel Linux, `/proc`, `/sys`, systemd, D-Bus de sistema, ALSA, módulos del kernel u otra infraestructura sin equivalente razonable para SST. | No portar salvo que exista una reimplementación funcional clara. |
| `NO APLICA` | Datos, temas, fuentes, plugins gráficos o paquetes que no aportan una herramienta CLI útil a SST. | Ignorar para `instp`. |

## Regla de adaptación

Un paquete no se considera adaptable solo porque compile en Rust, Go, C/C++ o Python. Para marcarlo `ADAPTABLE` hay que revisar al menos:

1. licencia compatible con redistribución/modificación;
2. repositorio upstream identificable;
3. interfaz CLI útil para SST;
4. dependencias de plataforma;
5. uso de `/proc`, `/sys`, `/dev`, señales POSIX, `fork`, PTY Unix, `ioctl`, systemd, D-Bus, X11/Wayland, ALSA/Pulse/JACK, netlink o módulos del kernel;
6. equivalentes Win32/Windows disponibles;
7. coste real del port frente a reimplementar la función en Nwash/Rust;
8. posibilidad de producir un `.exe` autocontenido o un paquete portable aislado.

## Qué entra en `instp`

`instp` debe distribuir herramientas ejecutables o runtimes. Si el upstream ya tiene Windows:

```json
{
  "id": "ejemplo",
  "name": "Ejemplo",
  "version": "latest",
  "category": "cli",
  "portability": "pure",
  "source": {
    "type": "github_release",
    "repository": "autor/proyecto",
    "asset_contains": ["windows", "x64"]
  },
  "install": {
    "type": "zip",
    "expose": ["ejemplo.exe"]
  }
}
```

Si SST mantiene un port propio:

```json
{
  "id": "ejemplo",
  "name": "Ejemplo (SST Windows port)",
  "version": "latest",
  "category": "cli",
  "portability": "pure",
  "source": {
    "type": "github_release",
    "repository": "glitchaka/ejemplo-windows"
  },
  "install": {
    "type": "zip",
    "expose": ["ejemplo.exe"]
  }
}
```

## Qué NO entra en `tools`

`tools` es el framework de diagnóstico, reparación y remoción de productos/sistema (`tools office ...`, futuros proveedores, etc.).

Las utilidades CLI portables pertenecen a `instp`, aunque hayan sido descubiertas revisando Arch Linux.

## Paquetes ya cubiertos por el modelo de SST

Estos proyectos aparecen en el ecosistema Arch y ya encajan en el catálogo portable de SST mediante builds Windows de sus upstreams:

| Paquete/proyecto | Estado | SST |
| --- | --- | --- |
| `ripgrep` | `DIRECTO` | ya está en `packages/index.json` (`rg.exe`) |
| `fd` | `DIRECTO` | ya está en `packages/index.json` |
| `bat` | `DIRECTO` | ya está en `packages/index.json` |
| `fzf` | `DIRECTO` | ya está en `packages/index.json` |
| `jq` | `DIRECTO` | ya está en `packages/index.json` |
| `yq` | `DIRECTO` | ya está en `packages/index.json` |
| `delta` | `DIRECTO` | ya está en `packages/index.json` |
| `hyperfine` | `DIRECTO` | ya está en `packages/index.json` |
| `tokei` | `DIRECTO` | ya está en `packages/index.json` |
| `bottom` | `DIRECTO` | ya está en `packages/index.json` (`btm.exe`) |
| `deno` | `DIRECTO` | ya está en `packages/index.json` |
| `bun` | `DIRECTO` | ya está en `packages/index.json` |
| `powershell` | `DIRECTO` | ya está en `packages/index.json` (`pwsh.exe`) |
| `gh` | `DIRECTO` | ya está en `packages/index.json` |
| `starship` | `DIRECTO` | ya está en `packages/index.json` |
| `zoxide` | `DIRECTO` | ya está en `packages/index.json` |
| `neovim` | `DIRECTO` | ya está en `packages/index.json` |
| `7zip` | `DIRECTO` | candidato para añadir a `instp`; upstream dispone de Windows |
| `act` | `DIRECTO` | candidato; revisar release Win64 y comportamiento Docker en Windows |
| `actionlint` | `DIRECTO` | candidato; CLI Go y upstream multiplataforma |
| `age` | `DIRECTO` | candidato; revisar selección de assets Win64 |

## Ejemplos que NO deben confundirse con un port sencillo

| Paquete Arch | Estado inicial | Motivo |
| --- | --- | --- |
| `acpi_call` / `acpi_call-lts` / `acpi_call-dkms` | `LINUX-ONLY` | módulo del kernel Linux y `/proc/acpi/call` |
| `acpid` | `LINUX-ONLY` | daemon de eventos ACPI/netlink de Linux |
| `accountsservice` | `LINUX-ONLY` | servicio de cuentas orientado al stack D-Bus/Linux |
| `a2jmidid` | `LINUX-ONLY` | ALSA sequencer/JACK |
| `acme-user` | `LINUX-ONLY` | paquete centrado en unidades/servicio systemd |
| `adwaita-*` | `NO APLICA` | temas, iconos, cursores o fuentes; no CLI SST |
| `abi-dumper` | `REVISAR` | trabaja con ELF/DWARF; habría que definir si aporta algo en PE/COFF Windows |
| `abduco` | `REVISAR` | gestión de sesión/TTY; revisar dependencia POSIX antes de considerar port |
| `aerc` | `REVISAR` | TUI compleja; revisar soporte upstream Windows y dependencias Unix |

## Criterio para escoger candidatos

Prioridad alta para SST:

- administración y diagnóstico;
- red, DNS, HTTP, TLS y sockets;
- filesystem y búsqueda;
- procesamiento de texto/JSON/YAML/CSV;
- seguridad defensiva y análisis;
- inspección de binarios/archivos;
- benchmarking y observabilidad;
- utilidades de desarrollo que funcionen bien desde consola;
- herramientas pequeñas que puedan reemplazar dependencias externas o mejorar Nwash.

Prioridad baja o nula:

- escritorios completos;
- juegos;
- temas/iconos/fuentes;
- drivers y módulos del kernel Linux;
- servicios profundamente atados a systemd;
- herramientas cuyo único propósito dependa de infraestructura Linux que SST no pretende emular.

## Flujo para revisar un paquete

```text
Arch package
    ↓
abrir upstream
    ↓
comprobar licencia
    ↓
¿upstream publica Windows?
    ├─ sí → DIRECTO → manifiesto instp
    └─ no
        ↓
    revisar dependencias POSIX/Linux
        ├─ reemplazables → ADAPTABLE → fork/port SST
        ├─ estructurales → LINUX-ONLY
        └─ incierto → REVISAR
```

## Objetivo

Este archivo es una lista de exploración, no una promesa de compatibilidad. Un paquete solo debe pasar de `REVISAR` a `DIRECTO` o `ADAPTABLE` después de revisar su upstream real. Así SST puede aprovechar el enorme catálogo de Arch como fuente de descubrimiento sin terminar otra vez envolviendo comandos de Windows para fingir semántica Bash.