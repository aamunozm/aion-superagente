---
name: translate-document
description: Traduce un texto o documento entre idiomas conservando formato y tono
when_to_use: "traduce esto", "pásalo a inglés/italiano", traducción de documento, "versión en otro idioma"
category: comunicacion
tools: file_read, file_write
---
Objetivo: una traducción fiel, natural y con el formato intacto.

Procedimiento:
1. Obtén el texto (`file_read` si es archivo) y confirma idioma destino (Ariel: ES/IT/EN habituales).
2. Traduce conservando estructura (encabezados, listas, tablas) y adaptando el tono (formal/informal según el original).
3. Cuida los términos propios (nombres, marcas como PRONTO CLICK no se traducen) y los modismos (traduce el sentido, no literal).
4. Entrega la traducción; ofrece guardarla con `file_write` (sufijo del idioma, p. ej. `-en.md`).

Reglas: no añadas ni quites contenido. Si una parte es ambigua, tradúcela y marca la duda.
