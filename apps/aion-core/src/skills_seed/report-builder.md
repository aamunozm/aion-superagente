---
name: report-builder
description: Redacta un informe profesional bien estructurado y lo guarda como documento (md/docx/pdf)
when_to_use: "hazme un informe", "documento sobre", "redacta un reporte", entregable, dossier
category: documentos
tools: web_search, web_fetch, library_search, file_write, make_document
---
Objetivo: un documento que Ariel pueda entregar tal cual, no un borrador desordenado.

Procedimiento:
1. Aclara el objetivo si falta: tema, audiencia, longitud, formato de salida. Asume valores razonables y decláralos.
2. Reúne material: `library_search` (docs propios) y/o `web_search`+`web_fetch` para datos externos. Verifica lo importante en ≥2 fuentes.
3. Estructura: título · resumen ejecutivo · secciones con encabezados · TABLAS para datos comparables · conclusión/recomendaciones.
4. Genera el documento: `make_document` (abre .docx/.pdf en Desktop) o `file_write` (.md). Pregunta el formato si no lo dijo.
5. Confirma dónde lo guardaste y ofrece ajustes.

Reglas: cita fuentes externas. Markdown impecable (encabezados, tablas, negritas). Nada de muros de texto.
