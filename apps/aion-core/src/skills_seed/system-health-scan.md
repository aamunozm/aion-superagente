---
name: system-health-scan
description: Diagnóstico completo del Mac (CPU, RAM, disco, térmica, batería, carga) con veredicto y recomendaciones
when_to_use: "cómo está mi mac", "escanea el sistema", "scanner completo", lentitud, rendimiento, qué consume CPU/memoria
category: sistema
tools: run_command
---
Objetivo: dar a Ariel una foto clara y accionable del estado de su Mac.

Procedimiento:
1. Ejecuta UN solo `run_command` que recoja todo de una vez (evita varias confirmaciones):
   `ps aux | sort -nrk 3,3 | head -15; echo '---MEM---'; vm_stat; echo '---DISCO---'; df -h /; echo '---CARGA---'; uptime; echo '---TERMICA---'; pmset -g therm 2>/dev/null; echo '---BATERIA---'; pmset -g batt 2>/dev/null`
2. Interpreta la salida, NO la pegues cruda. Traduce a lo que significa para él.
3. Estructura la respuesta: CPU (top procesos con nombre legible) · RAM (¿hay swapping/compresión? ¿al límite?) · Disco (% libre) · Térmica (¿throttling?) · Batería.
4. Da un veredicto por subsistema (✅/⚠️/🔴) y un resumen ejecutivo con 2-3 recomendaciones concretas (qué cerrar, qué revisar).

Reglas: si un proceso pesado es el propio AION/Ollama, dilo con honestidad. No alarmes sin datos.
