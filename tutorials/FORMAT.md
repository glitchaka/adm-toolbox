# Estándar de tutoriales SST

Los tutoriales de `adm-toolbox/tutorials` son documentación operativa para usuarios que reciben SST sin acompañamiento adicional.

Un tutorial no debe limitarse a mostrar uno o dos comandos. Debe permitir que una persona entienda qué puede hacer la herramienta, cuándo usarla, qué requisitos tiene, qué modifica y cómo verificar el resultado.

## Regla principal: cobertura exhaustiva

Cada tutorial debe documentar **todas y cada una de las opciones públicas realmente implementadas** para la herramienta correspondiente.

Esto incluye obligatoriamente:

- todos los comandos
- todos los subcomandos
- todos los aliases públicos
- todas las opciones cortas y largas
- todos los parámetros posicionales
- todos los valores aceptados
- todos los valores por defecto
- todas las combinaciones válidas relevantes
- todas las diferencias de comportamiento entre opciones
- todas las acciones destructivas o que cambien estado
- todos los estados de salida documentables
- todos los códigos de error que SST interprete explícitamente

No se permite resumir una familia de opciones con expresiones como "otras opciones", "etc." o "las opciones habituales" si esas opciones existen en el parser real.

Los helpers internos que SST utiliza para elevar o transportar operaciones, por ejemplo nombres internos del tipo `__elevated`, no forman parte de la interfaz pública y no deben presentarse al usuario como comandos utilizables. Sí puede explicarse su existencia a nivel interno cuando sea relevante para entender seguridad o elevación.

## Fuente de verdad

La documentación debe auditarse contra el código real de SST.

Para cada tutorial hay que revisar el parser y la implementación correspondiente y comprobar que:

1. cada subcomando público aparece documentado
2. cada flag aceptado por el parser aparece documentado
3. cada alias aceptado aparece documentado
4. cada valor enumerado o normalizado aparece documentado
5. los defaults coinciden con el código
6. las restricciones coinciden con la validación real
7. ninguna capacidad futura se presenta como existente
8. ninguna capacidad existente queda fuera del tutorial

Si el parser cambia, el tutorial debe actualizarse en el mismo ciclo de mantenimiento.

## Cobertura mínima obligatoria

Cada tutorial debe cubrir, cuando corresponda:

1. Objetivo de la herramienta.
2. Casos de uso reales.
3. Todos los comandos y aliases disponibles en SST.
4. Sintaxis completa.
5. Todos los parámetros y valores aceptados.
6. Valores por defecto y comportamiento cuando se omiten parámetros opcionales.
7. Ejemplos de uso simples y avanzados para cada modo importante.
8. Qué hace SST internamente a un nivel útil para diagnóstico.
9. Qué componentes de Windows o herramientas externas utiliza.
10. Requisitos previos.
11. Permisos y elevación UAC.
12. Efectos sobre el sistema.
13. Reinicios requeridos.
14. Cómo comprobar que la operación funcionó.
15. Estados y salidas que puede devolver SST.
16. Códigos de error relevantes cuando existan.
17. Problemas habituales y cómo diagnosticarlos.
18. Seguridad y manejo de credenciales o datos sensibles.
19. Qué NO hace la herramienta.
20. Relación con otros comandos SST.
21. Flujos de trabajo recomendados.
22. Limitaciones actuales.
23. Resumen rápido al final.

## Reglas de exactitud

- Documentar solo capacidades realmente implementadas.
- No presentar funciones futuras como si ya existieran.
- Si una función está planificada, debe indicarse explícitamente como futura.
- Los nombres de comandos deben coincidir exactamente con SST.
- Los parámetros deben coincidir exactamente con el parser real.
- Los aliases deben coincidir exactamente con los registrados por SST.
- Los valores por defecto deben coincidir con el código.
- Cuando SST use una API de Windows o una herramienta externa, debe indicarse.
- Si una acción requiere reinicio, debe quedar visible tanto en el flujo principal como en el resumen.
- Si una operación puede afectar dominio, identidad, credenciales, datos o disponibilidad del equipo, debe explicarse el impacto.
- Si una opción existe pero tiene una limitación concreta, esa limitación debe documentarse junto a la opción.

## Seguridad

Los tutoriales deben advertir claramente cuando:

- una contraseña aparece en la línea de comando o historial
- se solicita UAC
- se modifican objetos de Active Directory
- se eliminan aplicaciones o componentes
- una operación puede dejar al usuario sin acceso al equipo
- se descargan binarios externos
- SST valida firmas, hashes u otros controles de integridad

## Verificación posterior

Toda operación de cambio debe incluir una sección que indique cómo validar el resultado desde SST siempre que exista un comando para hacerlo.

Ejemplos:

- `hostname` después de renombrar
- `sys domain status` después de join/leave
- `tools office diagnose` después de scrub
- reinicio y prueba de sesión después de Autologon

## Organización

Los tutoriales deben escribirse en texto legible por terminal y evitar depender de HTML, imágenes o enlaces como única fuente de información.

El contenido puede alojarse remotamente y mostrarse en `guide`, pero debe seguir siendo comprensible como texto plano.

## Objetivo final

Una persona que reciba SST debe poder abrir `guide`, explorar la herramienta y comprender por sí sola:

- qué comandos existen
- para qué sirven
- todas las opciones disponibles
- qué valores acepta cada opción
- qué ocurre si omite un parámetro opcional
- cómo ejecutar cada modo correctamente
- qué riesgos implica cada operación
- cómo confirmar que funcionó
- qué hacer cuando falla
