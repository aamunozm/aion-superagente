---
name: find-large-files
description: Encuentra los archivos y carpetas que más espacio ocupan, para liberar disco
when_to_use: "qué ocupa espacio", "archivos grandes", "se me llena el disco", liberar espacio
category: sistema
tools: run_command
---
Objetivo: identificar dónde se está yendo el espacio, sin borrar nada por tu cuenta.

Procedimiento:
1. Carpetas más pesadas del HOME (un comando):
   `du -sh ~/* ~/Library/* 2>/dev/null | sort -rh | head -20`
2. Si Ariel señaló una carpeta concreta, foca ahí: `du -ah <carpeta> 2>/dev/null | sort -rh | head -25`
3. Archivos individuales grandes (>500 MB):
   `find ~ -type f -size +500M 2>/dev/null -exec ls -lh {} \; | awk '{print $5, $9}' | head -20`
4. Presenta una tabla ordenada por tamaño con rutas claras y un total aproximado liberable.

Reglas: NUNCA borres en esta skill. Si Ariel quiere borrar, propón qué y pide confirmación explícita (o usa la skill disk-cleanup). No toques rutas de sistema ni de credenciales.
