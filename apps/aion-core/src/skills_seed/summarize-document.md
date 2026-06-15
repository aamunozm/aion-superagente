---
name: summarize-document
description: Resume un documento o archivo largo en puntos clave y acciones
when_to_use: "resume este archivo", "qué dice el documento", "tldr de", resumen de PDF/nota/código
category: investigacion
tools: file_read, library_search
---
Objetivo: convertir un documento largo en algo accionable en 30 segundos de lectura.

Procedimiento:
1. Obtén el contenido: `file_read` con la ruta, o `library_search` si es un doc ya ingerido en la biblioteca.
2. Si está truncado (>8000 chars), pídelo por partes o céntrate en las secciones que Ariel necesita.
3. Entrega: TL;DR (2-3 frases) · puntos clave (viñetas) · datos/cifras relevantes · acciones o decisiones pendientes si las hay.
4. Adapta la longitud a lo que pida; por defecto, breve.

Reglas: no inventes contenido que no esté en el documento. Si algo es ambiguo, dilo en vez de rellenar.
