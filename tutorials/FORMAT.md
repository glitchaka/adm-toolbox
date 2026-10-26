# Estándar de tutoriales SST

Los tutoriales de `adm-toolbox/tutorials` son documentación operativa para usuarios que reciben SST sin acompañamiento adicional.

Un tutorial no debe limitarse a mostrar uno o dos comandos. Debe permitir que una persona entienda qué puede hacer la herramienta, cuándo usarla, qué requisitos tiene, qué modifica y cómo verificar el resultado.

## Cobertura mínima obligatoria

Cada tutorial debe cubrir, cuando corresponda:

1. Objetivo de la herramienta.
2. Casos de uso reales.
3. Todos los comandos y alias disponibles en SST.
4. Sintaxis completa.
5. Todos los parámetros y valores aceptados.
6. Ejemplos de uso simples y avanzados.
7. Qué hace SST internamente a un nivel útil para diagnóstico.
8. Qué componentes de Windows o herramientas externas utiliza.
9. Requisitos previos.
10. Permisos y elevación UAC.
11. Efectos sobre el sistema.
12. Reinicios requeridos.
13. Cómo comprobar que la operación funcionó.
14. Estados y salidas que puede devolver SST.
15. Códigos de error relevantes cuando existan.
16. Problemas habituales y cómo diagnosticarlos.
17. Seguridad y manejo de credenciales o datos sensibles.
18. Qué NO hace la herramienta.
19. Relación con otros comandos SST.
20. Flujos de trabajo recomendados.
21. Limitaciones actuales.
22. Resumen rápido al final.

## Reglas de exactitud

- Documentar solo capacidades realmente implementadas.
- No presentar funciones futuras como si ya existieran.
- Si una función está planificada, debe indicarse explícitamente como futura.
- Los nombres de comandos deben coincidir con SST.
- Los parámetros deben coincidir con el parser real.
- Cuando SST use una API de Windows o una herramienta externa, debe indicarse.
- Si una acción requiere reinicio, debe quedar visible tanto en el flujo principal como en el resumen.
- Si una operación puede afectar dominio, identidad, credenciales, datos o disponibilidad del equipo, debe explicarse el impacto.

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
- qué capacidades tienen
- cómo ejecutarlos correctamente
- qué riesgos implican
- cómo confirmar que funcionaron
- qué hacer cuando fallan
