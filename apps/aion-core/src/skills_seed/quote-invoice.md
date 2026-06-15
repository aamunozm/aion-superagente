---
name: quote-invoice
description: Genera una cotización o factura con partidas, subtotales, impuestos y total, lista como documento
when_to_use: "hazme una factura", "cotización", "presupuesto con precios", invoice, recibo
category: negocio
tools: make_document, file_write
---
Objetivo: un documento de cobro correcto y presentable (PRONTO CLICK).

Procedimiento:
1. Reúne: datos del cliente, lista de conceptos (descripción · cantidad · precio unitario), impuestos aplicables (p. ej. IVA), nº de documento y fecha. Pregunta lo que falte.
2. Calcula: subtotal por línea, subtotal, impuestos y TOTAL. Revisa la aritmética (usa la tool calculator si hace falta).
3. Estructura: encabezado (emisor PRONTO CLICK + cliente) · TABLA de partidas | Concepto | Cant. | Precio | Importe | · subtotal/impuestos/total · condiciones de pago.
4. Genera con `make_document` (.docx/.pdf) y confirma.

Reglas: NO inventes cifras; usa solo las que dé Ariel. Verifica que los totales cuadran antes de entregar.
