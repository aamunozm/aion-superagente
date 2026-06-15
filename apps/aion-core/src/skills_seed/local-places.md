---
name: local-places
description: Encuentra y compara sitios cercanos (restaurantes, tiendas, servicios) con datos útiles
when_to_use: "dónde como cerca", "busca un sitio para", "qué hay en esta dirección", restaurantes/tiendas cerca
category: personal
tools: place_lookup, web_search, weather
---
Objetivo: recomendaciones concretas y comparables, no una lista vaga.

Procedimiento:
1. Confirma zona (por defecto, Milano/contexto de Ariel) y qué busca.
2. `place_lookup` para identificar negocios en una dirección; `web_search` para opciones, valoraciones y horarios.
3. Si es al aire libre, mira el `weather`.
4. Presenta 3-5 opciones en TABLA | Sitio | Tipo | Distancia/Zona | Por qué | y una recomendación.

Reglas: verifica horarios en la fuente. Distingue dato de impresión. No inventes valoraciones.
