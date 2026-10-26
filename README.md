# SST Tools

Repositorio canónico de herramientas auxiliares y manuales operativos de Shell Shock Tool (SST).

Este repositorio ya no contiene el antiguo proyecto `adm-toolbox`. Su rama `main` cumple dos funciones para SST:

1. catálogo controlado de herramientas externas que SST puede localizar, descargar, verificar y ejecutar;
2. backend remoto de documentación exhaustiva consumida directamente por `guide`.

## Herramientas externas

- No se almacenan binarios propietarios de terceros en el repositorio.
- Cada herramienta tiene un descriptor JSON bajo `tools/`.
- Las descargas deben usar HTTPS.
- Cuando el ejecutable tenga firma Authenticode, SST debe validarla antes de ejecutarlo.
- Una herramienta archivada o proveniente de un mirror debe declararlo explícitamente.
- Los datos del usuario no forman parte de los paquetes de herramientas.

`catalog.json` es el índice estable de herramientas.

### Office

- `office.sara-legacy`: SaRACmd Enterprise legado para `OfficeScrubScenario`. SST lo descarga bajo demanda y exige firma Authenticode válida de Microsoft en `SaRAcmd.exe`.
- `office.gethelp`: backend actual `GetHelpCmd` de Microsoft para `OfficeScrubScenario`.

### Sysinternals

- `sysinternals-autologon`: Microsoft Sysinternals Autologon descargado desde Microsoft y verificado antes de ejecutarse desde SST.

## Manuales de SST

Los manuales viven en `tutorials/` y se mantienen separados del binario principal para poder ampliar o corregir documentación sin incrustar miles de líneas de texto en cada build.

`guide` los obtiene por HTTPS y los mantiene en memoria. No se guardan como archivos locales de documentación.

El índice es:

```text
tutorials/index.txt
```

El contrato editorial es:

```text
tutorials/FORMAT.md
```

### Cobertura

La documentación no está limitada a las herramientas externas. Debe abarcar toda la superficie pública de SST:

- arranque de sst.exe/Nwash;
- `notsh` / `net` completo;
- inventario `device`, switches y Wake-on-LAN;
- diagnósticos;
- `sys` y todos sus aliases/subcomandos;
- dominio, hostname y Autologon;
- seguridad (`triage`, `intel`, broker y persistencia);
- administración Windows/Nwash (`eventlog`, `service`, `registry`, `process`, `acl`, `pnp`, `task`, `session`, `share`, `firewall`, `power`);
- privilegios (`sudo` / `runas`);
- paquetes `instp`;
- configuración y fondos;
- actualización de SST;
- `tools` y Office;
- tour;
- Helix-SST;
- builtins Bash/Nwash;
- comandos Unix/GNU-like reimplementados.

### Regla de exhaustividad

Un manual no se considera completo si resume una familia con `etc.`, `[OPCIONES]` o texto equivalente en lugar de enumerar la interfaz real.

Cada capítulo debe auditarse contra el parser/código vigente y documentar:

- todos los subcomandos públicos;
- todos los aliases públicos;
- cada flag corto y largo implementado;
- valores aceptados;
- defaults;
- combinaciones válidas;
- permisos/UAC;
- efectos y reinicios;
- estados/códigos relevantes;
- verificación posterior;
- limitaciones y diferencias frente a la herramienta original cuando SST implemente solo un subconjunto.

Helpers internos como `__elevated` o `--*-internal` no forman parte de la interfaz del usuario y no se presentan como funciones normales.

Si una ayuda heredada anuncia una opción que el parser actual no implementa realmente, el manual debe decirlo en lugar de fingir compatibilidad.

## Uso desde SST

La puerta de entrada es:

```text
guide
```

También se puede abrir un comando o capítulo directamente:

```text
guide net
guide sys
guide autologon
guide windows-admin
guide bash-nwash
guide unix-compat
guide office-sara
```

`guide` mantiene una referencia local como fallback cuando existe, pero intenta mostrar el manual completo remoto cuando hay conectividad.

La categoría `Tutoriales` se genera desde el catálogo de manuales para que una persona que recibe SST pueda explorar las capacidades sin conocer previamente los nombres de los comandos.

Cuando se implemente activación/licencia en SST, el transporte de estos manuales podrá exigir una activación válida sin cambiar la interfaz de `guide` ni los nombres de los capítulos.

El código principal de SST permanece en `glitchaka/sst-blueversion`.
