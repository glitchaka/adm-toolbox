# SST Tools

Repositorio canónico de herramientas auxiliares para Shell Shock Tool (SST).

Este repositorio ya no contiene el antiguo proyecto `adm-toolbox`. Su rama `main` es un catálogo limpio de herramientas externas que SST puede localizar, descargar y ejecutar de forma controlada.

## Reglas

- No se almacenan binarios propietarios de terceros en el repositorio.
- Cada herramienta tiene un descriptor JSON bajo `tools/`.
- Las descargas deben usar HTTPS.
- Cuando el ejecutable tenga firma Authenticode, SST debe validarla antes de ejecutarlo.
- Una herramienta archivada o proveniente de un mirror debe declararlo explícitamente.
- Los datos del usuario no forman parte de los paquetes de herramientas.

## Catálogo

`catalog.json` es el índice estable que SST puede consultar. Los descriptores individuales contienen versión, origen, ejecutable, parámetros y requisitos de verificación.

### Office

- `office.sara-legacy`: SaRACmd Enterprise legado para `OfficeScrubScenario`. El paquete no se redistribuye aquí; SST lo descarga bajo demanda y exige firma Authenticode válida de Microsoft en `SaRAcmd.exe`.
- `office.gethelp`: backend actual `GetHelpCmd` de Microsoft para `OfficeScrubScenario`.

El código principal de SST permanece en `glitchaka/sst-blueversion`.
