# SST Tools

Repositorio canónico de herramientas auxiliares y tutoriales para Shell Shock Tool (SST).

Este repositorio ya no contiene el antiguo proyecto `adm-toolbox`. Su rama `main` es un catálogo limpio de herramientas externas que SST puede localizar, descargar y ejecutar de forma controlada, junto con documentación remota consumida directamente desde SST.

## Reglas

- No se almacenan binarios propietarios de terceros en el repositorio.
- Cada herramienta tiene un descriptor JSON bajo `tools/`.
- Las descargas deben usar HTTPS.
- Cuando el ejecutable tenga firma Authenticode, SST debe validarla antes de ejecutarlo.
- Una herramienta archivada o proveniente de un mirror debe declararlo explícitamente.
- Los datos del usuario no forman parte de los paquetes de herramientas.
- Los tutoriales viven bajo `tutorials/` y SST los consume en memoria; el visor no los guarda como archivos locales.
- Cuando se implemente activación/licencia en SST, el acceso remoto a tutoriales quedará condicionado a una activación válida sin cambiar los comandos de usuario.

## Catálogo

`catalog.json` es el índice estable que SST puede consultar. Los descriptores individuales contienen versión, origen, ejecutable, parámetros y requisitos de verificación.

### Office

- `office.sara-legacy`: SaRACmd Enterprise legado para `OfficeScrubScenario`. El paquete no se redistribuye aquí; SST lo descarga bajo demanda y exige firma Authenticode válida de Microsoft en `SaRAcmd.exe`.
- `office.gethelp`: backend actual `GetHelpCmd` de Microsoft para `OfficeScrubScenario`.

### Sysinternals

- `sysinternals-autologon`: Microsoft Sysinternals Autologon descargado desde Microsoft y verificado antes de ejecutarse desde SST.

## Tutoriales

El índice remoto está en `tutorials/index.txt`.

Actualmente incluye:

- `office-sara`
- `office-gethelp`
- `autologon`
- `domain`
- `hostname`

Desde SST:

```text
tools tutorial list
tools tutorial office-sara
tools tutorial office-gethelp
tools tutorial autologon
tools tutorial domain
tools tutorial hostname
```

SST lee el contenido remoto a memoria y lo imprime en la consola. El lector limita los nombres de tutorial y el tamaño máximo del contenido para evitar usarlo como un fetch arbitrario.

El código principal de SST permanece en `glitchaka/sst-blueversion`.
