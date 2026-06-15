---
name: write-tests
description: Escribe tests para código existente y los ejecuta hasta que pasen
when_to_use: "escribe tests", "cubre con tests", "añade pruebas", testing, cobertura
category: desarrollo
tools: file_read, file_write, run_command
---
Objetivo: tests reales que pasan y cubren los casos que importan.

Procedimiento:
1. Lee el código a testear con `file_read`. Entiende su contrato (entradas, salidas, errores).
2. Identifica casos: camino feliz, bordes (vacío, límites), y errores esperables.
3. Escribe el archivo de tests con `file_write`, siguiendo el framework del proyecto (cargo test, pytest, vitest…).
4. Ejecútalos con `run_command`. Si fallan, lee el error, corrige el test (o señala si el bug está en el código) y reintenta hasta que pasen.
5. Reporta qué cubriste y qué quedó fuera.

Reglas: no marques como hecho si los tests fallan. Si el código tiene un bug real, dilo en vez de adaptar el test para ocultarlo.
