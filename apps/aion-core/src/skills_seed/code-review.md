---
name: code-review
description: Revisa código en busca de bugs, problemas de seguridad y mejoras, con severidad y fix
when_to_use: "revisa este código", "hay bugs", code review, "qué mejorarías", auditar archivo de código
category: desarrollo
tools: file_read, run_command, file_write
---
Objetivo: una revisión accionable, no estética.

Procedimiento:
1. Lee el/los archivo(s) con `file_read`. Si es un proyecto, usa `run_command` con `git diff`, `ls`, o `grep -rn` para acotar lo cambiado/relevante.
2. Analiza en 3 ejes: 🔴 Bugs/correctitud · 🟠 Seguridad (inyección, secretos, validación) · 🟡 Calidad/rendimiento.
3. Por hallazgo: archivo:línea · severidad · por qué es un problema · fix concreto.
4. Sé adversarial pero verifica contra el código real (no inventes líneas). Distingue lo seguro de lo dudoso.
5. Ofrece guardar el informe con `file_write` si es largo.

Reglas: prioriza pocos hallazgos de alta confianza sobre muchos inciertos. No reescribas todo; señala y propón.
