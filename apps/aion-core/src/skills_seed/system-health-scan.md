---
name: system-health-scan
description: Diagnóstico experto y completo del Mac (hardware, CPU, RAM, disco, GPU, red, térmica, batería con salud) — veredicto por subsistema y recomendaciones priorizadas
when_to_use: "cómo está mi mac", "escanea el sistema", "scanner completo", "verifica mi macbook", "procesos y aplicaciones activas", lentitud, rendimiento, batería, temperatura
category: sistema
tools: run_command
---
Actúa como un INGENIERO SENIOR de rendimiento de macOS. Objetivo: una radiografía completa y accionable del Mac de Ariel, en UNA sola pasada (una sola confirmación).

## Paso 1 — Recoger TODO con UN SOLO `run_command`
Usa EXACTAMENTE este comando (usa `comm`, no `ps aux`, para no truncar con argumentos largos):

```
echo '### HW'; system_profiler SPHardwareDataType 2>/dev/null | grep -iE 'Model Name|Chip|Total Number of Cores|^ *Memory:'; sw_vers | tr '\n' ' '; echo; echo '### CARGA'; uptime; echo '### CPU'; ps -Ao pid,pcpu,pmem,comm | sort -k2 -rn | head -12; echo '### MEM'; vm_stat; echo '### DISCO'; df -h / /System/Volumes/Data 2>/dev/null; echo '### RED'; echo "IP: $(ipconfig getifaddr en0 2>/dev/null)"; echo "conexiones ESTABLISHED: $(netstat -an 2>/dev/null | grep -c ESTABLISHED)"; echo '### GPU'; system_profiler SPDisplaysDataType 2>/dev/null | grep -iE 'Chipset|VRAM|Metal|Resolution' | head -5; echo '### TERMICA'; pmset -g therm 2>/dev/null; echo '### BATERIA'; pmset -g batt 2>/dev/null | tail -1; system_profiler SPPowerDataType 2>/dev/null | grep -iE 'Cycle Count|Condition|Maximum Capacity|Fully Charged'; echo '### APPS'; osascript -e 'tell application "System Events" to get name of (processes where background only is false)' 2>/dev/null
```

Si por lo que sea una sección saliera vacía, NO repitas todo: sigue con lo que tengas y dilo.

## Paso 2 — Interpretar como experto (NO pegar la salida cruda)
- **CPU**: nombres legibles; quién suma más; relación con el *load average* (si supera el nº de núcleos de forma sostenida → saturación). Marca procesos anómalos.
- **RAM**: page size = 16 KB → convierte páginas a GB. Uso real ≈ (active + wired); liberable ≈ (inactive + purgeable). PRESIÓN: si «pages stored in compressor» es alto y hay muchos swapins/swapouts → el sistema comprime/intercambia (ralentiza). Estima uso vs RAM total (del bloque HW).
- **Disco**: usa el % del volumen de **Datos**, no solo el snapshot del sistema.
- **GPU**: chip y memoria/Metal.
- **Térmica**: ¿throttling o advertencias? Sin sudo NO hay °C exactos — dilo, no lo inventes.
- **Batería**: % y carga + **ciclos** y **condición / capacidad máxima** (salud real, no solo el %). Ciclos altos o condición ≠ «Normal» → menciónalo.
- **Apps**: lista legible de lo abierto.

## Paso 3 — Presentar SIEMPRE así (Markdown → se renderiza en tablas)
- Un encabezado `##` por subsistema con su **veredicto**: ✅ bien · 🟡 atención · 🔴 crítico.
- **TABLAS** para procesos y métricas: | Indicador | Valor | Qué significa |.
- **Resumen ejecutivo** final: estado global + 2-4 **recomendaciones priorizadas y concretas** (qué cerrar/revisar y en qué orden).

Reglas: si un proceso pesado eres TÚ (aion-desktop / ollama / llama-server / WebKit de AION), dilo con honestidad. No alarmes sin datos. Lo que no puedas medir, decláralo en una línea — nunca lo inventes.
