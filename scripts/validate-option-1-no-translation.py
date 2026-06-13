#!/usr/bin/env python3
"""
Validación DÍA 1: Opción 1 (Sin traducción)

¿Usuarios españoles aceptan respuestas en inglés técnico?

Genera 5 pares: pregunta en español → respuesta en inglés comprimido
Métricas: claridad, coherencia, aceptabilidad
"""

import json
from datetime import datetime

# Pares pregunta-respuesta simulados (español → inglés comprimido)
TEST_PAIRS = [
    {
        "id": 1,
        "pregunta_es": "¿Cómo configuro la autenticación en la API?",
        "respuesta_en": "To configure API authentication: (1) Generate an access token via POST /auth/token with your credentials. (2) Include token in Authorization header: 'Bearer <token>'. (3) Tokens expire after 24h; use refresh endpoint. (4) Scopes control permissions: 'read', 'write', 'admin'. (5) For testing, use sandbox keys in docs.",
        "dominio": "API Configuration",
    },
    {
        "id": 2,
        "pregunta_es": "¿Cuál es el límite de requests por minuto?",
        "respuesta_en": "Rate limits per tier: Free (100 req/min), Pro (1000 req/min), Enterprise (unlimited). Current usage visible in X-RateLimit-Remaining header. If exceeded, retry after 60s. Contact support for tier upgrade.",
        "dominio": "Rate Limiting",
    },
    {
        "id": 3,
        "pregunta_es": "¿Cómo manejar errores en mis integraciones?",
        "respuesta_en": "Error responses include HTTP status + JSON body: {error: 'code', message: 'detail'}. Common codes: 400 (invalid request), 401 (auth failed), 429 (rate limited), 500 (server error). Retry logic: exponential backoff starting 1s. Idempotency key prevents duplicates.",
        "dominio": "Error Handling",
    },
    {
        "id": 4,
        "pregunta_es": "¿Dónde está documentado el webhook payload?",
        "respuesta_en": "Webhooks send POST requests to your endpoint with event data. Payload format in docs/webhooks.md. Signature verification: HMAC-SHA256(secret, body). Retry policy: exponential backoff, 24h max. Test events via dashboard Events tab.",
        "dominio": "Webhooks",
    },
    {
        "id": 5,
        "pregunta_es": "¿Cuáles son las regiones disponibles?",
        "respuesta_en": "Available regions: us-east (default, lowest latency), eu-west, ap-southeast. Specify region in config or X-Region header. Data residency: requests stay in selected region. Latency: 50-200ms avg. Check region status: GET /status/regions.",
        "dominio": "Infrastructure",
    },
]

print("=" * 80)
print("DÍA 1: VALIDACIÓN OPCIÓN 1 (Sin traducción)")
print("¿Usuarios españoles aceptan respuestas en inglés técnico?")
print("=" * 80)

print("\n📋 INSTRUCCIONES PARA USER TEST:")
print("""
Para cada par (pregunta en español → respuesta en inglés):

1. Lee la PREGUNTA (en español)
2. Lee la RESPUESTA (en inglés técnico)
3. Responde:
   a) ¿Entiendes la respuesta? (1=nada, 5=perfectamente)
   b) ¿Preferirías español? (Sí/No)
   c) ¿La encontraste útil? (Sí/No)

Objetivo: Medir aceptabilidad de inglés técnico como fallback default.
""")

print("\n" + "=" * 80)
print("5 PARES PREGUNTA-RESPUESTA PARA TESTING")
print("=" * 80)

results = {
    "timestamp": datetime.now().isoformat(),
    "test_pairs": [],
    "instructions": "User debe responder 3 preguntas por par (1-5, Sí/No, Sí/No)",
}

for pair in TEST_PAIRS:
    print(f"\n{'─' * 80}")
    print(f"PAR #{pair['id']} — {pair['dominio']}")
    print(f"{'─' * 80}")

    print(f"\n🇪🇸 PREGUNTA (Español):")
    print(f"   {pair['pregunta_es']}")

    print(f"\n🇬🇧 RESPUESTA (English - Technical):")
    print(f"   {pair['respuesta_en']}")

    print(f"\n📝 USER TEST — Responde:")
    print(f"   a) ¿Entiendes? (1=nada, 5=perfectamente) → [?]")
    print(f"   b) ¿Preferirías español? (Sí/No) → [?]")
    print(f"   c) ¿Útil? (Sí/No) → [?]")
    print()

    # Placeholder para resultados
    results["test_pairs"].append({
        "pair_id": pair["id"],
        "dominio": pair["dominio"],
        "pregunta_es": pair["pregunta_es"],
        "respuesta_en": pair["respuesta_en"],
        "usuario_response": {
            "comprension": None,  # 1-5
            "prefiere_espanol": None,  # True/False
            "util": None,  # True/False
        },
    })

print("\n" + "=" * 80)
print("MÉTRICAS A RECOPILAR")
print("=" * 80)

print("""
Después de que usuario complete el test, calcular:

1. COMPRENSIÓN PROMEDIO
   └─ Promedio de "¿Entiendes?" (escala 1-5)
      └─ Target: ≥4 (muy bien) para aceptar Opción 1

2. % PREFIERE ESPAÑOL
   └─ Cuántos contestaron "Sí, prefiero español"
      └─ Target: <50% (menos de la mitad necesita traducción)

3. % ÚTIL
   └─ Cuántos encontraron útil la respuesta
      └─ Target: >80% (es usable)

4. VEREDICTO
   └─ Si comprensión ≥4 AND prefiere_español <50%:
      ✅ Opción 1 es VIABLE como default
   └─ Si comprensión <4 OR prefiere_español ≥50%:
      ❌ Opción 3 (mBART) es OBLIGATORIA
""")

print("\n" + "=" * 80)
print("GUARDAR RESULTADOS")
print("=" * 80)

results_file = "/tmp/option-1-validation-results.json"
print(f"\nResultados guardados en: {results_file}")
print("Formato esperado para cada par:")
print(json.dumps(results["test_pairs"][0], indent=2))

# Guardar template
with open(results_file, "w") as f:
    json.dump(results, f, indent=2, ensure_ascii=False)

print(f"\n✅ Template guardado. Complétalo después del user test.")
print("\n" + "=" * 80)
print("SIGUIENTE PASO: User Test + Recopilación")
print("=" * 80)
print("""
1. Comparte estos 5 pares con 2-3 usuarios españoles
2. Recoge sus respuestas (comprensión, preferencia, utilidad)
3. Calcula promedios
4. Decide: ¿Opción 1 viable? → SÍ/NO

DÍA 2: Validar Opción 3 (mBART local)
└─ Si Opción 1 = SÍ viable: mBART es fallback opcional
└─ Si Opción 1 = NO viable: mBART es OBLIGATORIO
""")
