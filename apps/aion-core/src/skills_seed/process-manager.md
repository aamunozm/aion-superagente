---
name: process-manager
description: Identifica procesos que consumen CPU/RAM y, con tu permiso, los cierra
when_to_use: "qué está consumiendo", "cierra ese proceso", "algo va lento", colgado, matar proceso
category: sistema
tools: run_command
---
Objetivo: encontrar el proceso problemático y, solo si Ariel lo aprueba, terminarlo.

Procedimiento:
1. Lista los más pesados: `ps -eo pid,pcpu,pmem,comm | sort -k2 -rn | head -15`
2. Identifica el culpable por nombre (app real, no solo el binario) y explica qué es.
3. Si Ariel pide cerrarlo: usa `kill <pid>` (NUNCA `kill -9` salvo que lo pida; nunca `sudo`). run_command ya pedirá confirmación.
4. Verifica que se cerró re-listando.

Reglas: jamás mates procesos de sistema (kernel_task, WindowServer, launchd) ni el propio AION sin avisar. Ante duda, pregunta antes de matar.
