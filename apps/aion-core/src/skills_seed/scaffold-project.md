---
name: scaffold-project
description: Crea la estructura inicial de un proyecto (carpetas, archivos base, config) listo para arrancar
when_to_use: "crea un proyecto", "estructura inicial", scaffold, "empieza un repo", boilerplate
category: desarrollo
tools: run_command, file_write, file_read
---
Objetivo: dejar un proyecto que arranca, no un esqueleto vacío.

Procedimiento:
1. Confirma lo esencial si falta: lenguaje/stack, nombre, carpeta destino. Asume valores razonables y decláralos.
2. Crea la carpeta raíz con `run_command` (`mkdir -p`).
3. Escribe los archivos base con `file_write`: manifiesto (package.json/Cargo.toml/pyproject…), entrypoint, README, .gitignore, y un test mínimo.
4. Si aplica, inicializa git y deja un primer commit (`git init`, `git add -A`, `git commit`) — run_command pedirá confirmación.
5. Verifica que compila/arranca con el comando del stack (`cargo build`, `npm install`, etc.).
6. Resume qué creaste y el siguiente paso exacto para Ariel.

Reglas: no sobrescribas archivos existentes sin avisar. Sigue convenciones del stack elegido.
