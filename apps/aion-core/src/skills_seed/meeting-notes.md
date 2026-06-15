---
name: meeting-notes
description: Convierte notas o una transcripción de reunión en resumen, decisiones y acciones con responsable
when_to_use: "resume la reunión", "notas de la junta", "saca las acciones de", acta, minuta
category: documentos
tools: file_read, file_write
---
Objetivo: que tras la reunión quede claro QUÉ se decidió y QUIÉN hace QUÉ.

Procedimiento:
1. Obtén el material: `file_read` de las notas/transcripción, o tómalo de lo que pegó Ariel.
2. Extrae: temas tratados · decisiones tomadas · **acciones** (qué · responsable · fecha) · dudas abiertas.
3. Presenta una TABLA de acciones: | Acción | Responsable | Fecha |. Y un resumen breve arriba.
4. Ofrece guardarlo con `file_write` (p. ej. ~/Desktop/acta-<fecha>.md).

Reglas: no inventes acuerdos ni responsables que no estén en las notas. Marca como "pendiente de confirmar" lo ambiguo.
