# DÍA 1: Validación Opción 1 — User Test Template

**Objetivo:** Medir si usuarios españoles aceptan respuestas en inglés técnico

---

## 📋 Instrucciones para Usuarios

**Para cada par de pregunta-respuesta:**

1. Lee la **PREGUNTA** (en español)
2. Lee la **RESPUESTA** (en inglés técnico)
3. Responde las 3 preguntas de evaluación

---

## PAR 1 — API Configuration

**🇪🇸 Pregunta (Español):**
> ¿Cómo configuro la autenticación en la API?

**🇬🇧 Respuesta (English):**
> To configure API authentication: (1) Generate an access token via POST /auth/token with your credentials. (2) Include token in Authorization header: 'Bearer <token>'. (3) Tokens expire after 24h; use refresh endpoint. (4) Scopes control permissions: 'read', 'write', 'admin'. (5) For testing, use sandbox keys in docs.

**📝 Evaluación:**
- a) ¿Entiendes la respuesta? (1=nada, 5=perfectamente) → **[ ]**
- b) ¿Preferirías español? (Sí/No) → **[ ]**
- c) ¿La encontraste útil? (Sí/No) → **[ ]**

---

## PAR 2 — Rate Limiting

**🇪🇸 Pregunta (Español):**
> ¿Cuál es el límite de requests por minuto?

**🇬🇧 Respuesta (English):**
> Rate limits per tier: Free (100 req/min), Pro (1000 req/min), Enterprise (unlimited). Current usage visible in X-RateLimit-Remaining header. If exceeded, retry after 60s. Contact support for tier upgrade.

**📝 Evaluación:**
- a) ¿Entiendes la respuesta? (1=nada, 5=perfectamente) → **[ ]**
- b) ¿Preferirías español? (Sí/No) → **[ ]**
- c) ¿La encontraste útil? (Sí/No) → **[ ]**

---

## PAR 3 — Error Handling

**🇪🇸 Pregunta (Español):**
> ¿Cómo manejar errores en mis integraciones?

**🇬🇧 Respuesta (English):**
> Error responses include HTTP status + JSON body: {error: 'code', message: 'detail'}. Common codes: 400 (invalid request), 401 (auth failed), 429 (rate limited), 500 (server error). Retry logic: exponential backoff starting 1s. Idempotency key prevents duplicates.

**📝 Evaluación:**
- a) ¿Entiendes la respuesta? (1=nada, 5=perfectamente) → **[ ]**
- b) ¿Preferirías español? (Sí/No) → **[ ]**
- c) ¿La encontraste útil? (Sí/No) → **[ ]**

---

## PAR 4 — Webhooks

**🇪🇸 Pregunta (Español):**
> ¿Dónde está documentado el webhook payload?

**🇬🇧 Respuesta (English):**
> Webhooks send POST requests to your endpoint with event data. Payload format in docs/webhooks.md. Signature verification: HMAC-SHA256(secret, body). Retry policy: exponential backoff, 24h max. Test events via dashboard Events tab.

**📝 Evaluación:**
- a) ¿Entiendes la respuesta? (1=nada, 5=perfectamente) → **[ ]**
- b) ¿Preferirías español? (Sí/No) → **[ ]**
- c) ¿La encontraste útil? (Sí/No) → **[ ]**

---

## PAR 5 — Infrastructure

**🇪🇸 Pregunta (Español):**
> ¿Cuáles son las regiones disponibles?

**🇬🇧 Respuesta (English):**
> Available regions: us-east (default, lowest latency), eu-west, ap-southeast. Specify region in config or X-Region header. Data residency: requests stay in selected region. Latency: 50-200ms avg. Check region status: GET /status/regions.

**📝 Evaluación:**
- a) ¿Entiendes la respuesta? (1=nada, 5=perfectamente) → **[ ]**
- b) ¿Preferirías español? (Sí/No) → **[ ]**
- c) ¿La encontraste útil? (Sí/No) → **[ ]**

---

## 📊 Cómo Guardar Resultados

**Opción A: JSON (recomendado para análisis)**

```json
{
  "test_date": "2026-06-13",
  "tester_name": "[Tu nombre]",
  "results": [
    {
      "pair_id": 1,
      "comprension": 5,
      "prefiere_espanol": false,
      "util": true
    },
    {
      "pair_id": 2,
      "comprension": 4,
      "prefiere_espanol": false,
      "util": true
    },
    ...
  ]
}
```

**Opción B: Markdown (texto simple)**

```markdown
# Resultados User Test — [Tu nombre]

| Par | Dominio | Comprensión (1-5) | Prefiere Español? | Útil? |
|-----|---------|---|---|---|
| 1 | API Config | 5 | No | Sí |
| 2 | Rate Limiting | 4 | No | Sí |
| 3 | Error Handling | 5 | No | Sí |
| 4 | Webhooks | 4 | No | Sí |
| 5 | Infrastructure | 3 | Sí | Sí |
```

---

## 📈 Cálculo de Veredicto

**Después de completar todos los pares:**

```
1. Comprensión promedio = (suma todas) / 5
   Target: ≥4.0

2. % Prefiere español = (cuántos "Sí") / 5 × 100
   Target: <50%

3. % Útil = (cuántos "Sí") / 5 × 100
   Target: >80%

VEREDICTO:
├─ Si comprensión ≥4.0 AND prefiere_español <50%:
│  ✅ Opción 1 VIABLE
└─ Si no:
   ❌ Opción 3 (mBART) OBLIGATORIA
```

---

## 🚀 Próximo Paso

Completa el test, calcula promedios, y comparte veredicto:

- **Opción 1 viable** → Pasamos a Día 2 (validar mBART fallback)
- **Opción 1 no viable** → Pasamos a Día 2 (implementar mBART requerido)
