---
name: explain-codebase
description: Explica cómo está organizado y cómo funciona un proyecto de código
when_to_use: "explícame este código", "cómo funciona este repo", "qué hace este proyecto", entender codebase, arquitectura
category: desarrollo
tools: run_command, file_read
---
Objetivo: un mapa mental del proyecto que ahorre horas de lectura.

Procedimiento:
1. Vista general: `run_command` con `ls`, `find . -maxdepth 2 -type d`, y lee el README y el manifiesto (Cargo.toml/package.json) con `file_read`.
2. Localiza el entrypoint y los módulos clave (`grep -rn "fn main\|export default\|if __name__"`).
3. Lee los 2-4 archivos más centrales para entender el flujo principal.
4. Explica: propósito del proyecto · arquitectura (módulos y cómo se relacionan) · flujo de una operación típica · dónde tocar para X.
5. Usa rutas concretas (archivo:línea) para que Ariel pueda saltar ahí.

Reglas: básate en el código real leído, no en suposiciones por el nombre. Si algo no lo miraste, dilo.
