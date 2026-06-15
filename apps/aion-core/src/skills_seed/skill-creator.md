---
name: skill-creator
description: Crea una NUEVA skill (playbook) escribiendo su SKILL.md, para que AION aprenda a resolver una tarea repetible
when_to_use: "créate una skill para", "aprende a hacer esto siempre", "guarda este procedimiento", "para la próxima vez hazlo así"
category: meta
tools: file_write
---
Objetivo: que AION amplíe sus propias capacidades guardando un procedimiento reutilizable (auto-mejora real).

Procedimiento:
1. Define la skill: nombre en kebab-case, descripción de una línea, cuándo usarla (when_to_use), categoría, y qué tools usa.
2. Escribe las INSTRUCCIONES como un playbook claro: objetivo + pasos numerados + reglas/límites. Reusa SIEMPRE las tools existentes (no inventes herramientas).
3. Guarda el archivo con `file_write` en:
   `~/Library/Application Support/AION/skills_lib/<nombre>.md`
   con este formato exacto (frontmatter entre `---` + cuerpo):
   ```
   ---
   name: <nombre>
   description: <una línea>
   when_to_use: <pistas de disparo>
   category: <categoria>
   tools: tool_a, tool_b
   ---
   <instrucciones paso a paso>
   ```
4. Confirma a Ariel que la skill quedó creada; estará disponible en el catálogo la próxima vez (se relee del disco).

Reglas: una skill = un procedimiento concreto y seguro. No crees skills que hagan acciones destructivas sin confirmación. No dupliques una skill que ya existe (revisa el catálogo primero).
