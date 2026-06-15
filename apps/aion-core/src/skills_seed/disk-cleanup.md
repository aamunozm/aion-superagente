---
name: disk-cleanup
description: Propone y, con tu permiso, libera espacio seguro (cachés, descargas viejas, papelera, logs)
when_to_use: "libera espacio", "limpia el disco", "borra basura", caché, descargas viejas
category: sistema
tools: run_command, file_write
---
Objetivo: recuperar espacio SIN riesgo, siempre proponiendo antes de borrar.

Procedimiento:
1. Mide candidatos seguros (solo lectura):
   `echo '---CACHES---'; du -sh ~/Library/Caches 2>/dev/null; echo '---DESCARGAS---'; du -sh ~/Downloads 2>/dev/null; echo '---PAPELERA---'; du -sh ~/.Trash 2>/dev/null; echo '---LOGS---'; du -sh ~/Library/Logs 2>/dev/null`
2. Presenta una tabla con cuánto se liberaría por categoría y propón un plan.
3. Solo tras aprobación explícita de Ariel, borra lo acordado (run_command pedirá confirmación). Usa borrado por categoría, ej: vaciar papelera, limpiar ~/Library/Caches de apps concretas.
4. Verifica el espacio liberado con `df -h /` antes y después.

Reglas: NUNCA `sudo`, ni `rm -rf` de rutas amplias, ni tocar Documents/Desktop/Pictures ni datos de apps activas. En la duda, no borres y pregunta.
