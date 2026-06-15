---
name: market-scan
description: Panorama de un mercado o competencia: jugadores, precios, posicionamiento, oportunidades
when_to_use: "analiza la competencia", "cómo está el mercado de", precios del sector, benchmark, "quién más hace esto"
category: negocio
tools: web_search, web_fetch, file_write
---
Objetivo: una vista clara del terreno para decidir (precio, posicionamiento, oportunidad).

Procedimiento:
1. Define el mercado/segmento concreto y la geografía (por defecto, el contexto de Ariel: Italia).
2. `web_search`+`web_fetch`: identifica 4-8 competidores/ofertas y captura precio, propuesta de valor y público.
3. Sintetiza en una TABLA comparativa | Jugador | Precio | Propuesta | Notas |.
4. Cierra con: huecos/oportunidades detectados y una recomendación de posicionamiento.

Reglas: verifica precios en la fuente (no de memoria). Distingue dato de estimación. Ofrece guardar con `file_write`.
