---
name: data-analysis
description: Analiza un CSV/datos: estadísticas clave, patrones y hallazgos, con tablas
when_to_use: "analiza estos datos", "qué dice este CSV", estadísticas de, tendencias, "saca conclusiones de"
category: datos
tools: file_read, run_command, file_write
---
Objetivo: convertir datos crudos en hallazgos accionables.

Procedimiento:
1. Carga: `file_read` para ver cabeceras y muestra; si el archivo es grande, usa `run_command` con `head`, `wc -l`, `awk`/`sort`/`cut` para calcular sin cargarlo entero.
2. Entiende las columnas y su tipo. Calcula lo relevante: totales, medias, máximos/mínimos, conteos por categoría, tendencias.
3. Presenta: resumen de qué son los datos · TABLA con las métricas clave · 3-5 hallazgos en viñetas · anomalías.
4. Si pide visual, describe el gráfico recomendado; ofrece guardar el análisis con `file_write`.

Reglas: básate en los datos reales, no en suposiciones. Indica el tamaño de muestra y lo que no pudiste calcular.
