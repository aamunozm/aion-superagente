---
name: proposal-generator
description: Redacta una propuesta comercial (problema, solución, alcance, precio, plazos) lista para enviar
when_to_use: "hazme una propuesta", "presupuesto para el cliente", oferta comercial, cotización con texto
category: negocio
tools: file_write, make_document, web_search
---
Objetivo: una propuesta persuasiva y clara que Ariel pueda enviar con su marca (PRONTO CLICK).

Procedimiento:
1. Reúne lo necesario: cliente, problema a resolver, solución propuesta, alcance, precio/condiciones, plazos. Pregunta lo que falte (o asume y decláralo).
2. Estructura: portada/encabezado · contexto y problema · solución propuesta · alcance (qué incluye / qué no) · **inversión** (TABLA de partidas y precio) · plazos · siguientes pasos.
3. Tono profesional y directo, orientado a beneficio para el cliente.
4. Genera el documento con `make_document` (.docx/.pdf) o `file_write` (.md) y confirma dónde quedó.

Reglas: no inventes precios; si Ariel no los dio, deja placeholders claros para que los rellene. Coherente con la identidad de PRONTO CLICK.
