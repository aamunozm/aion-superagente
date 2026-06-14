#!/usr/bin/env python3
"""
Verificación end-to-end de mcp_compact: traduce memoria española real con Gemma LOCAL
(el mismo prompt que usa apps/aion-core/src/mcp_compact.rs) y mide el ahorro de tokens
ES→EN con tiktoken. Requiere Ollama corriendo con gemma4-reason.

Uso:  python3 scripts/verify_mcp_compact.py
"""
import json
import sys
import urllib.request

OLLAMA = "http://127.0.0.1:11434/api/generate"
MODEL = "gemma4-reason:latest"

# El MISMO prompt de mcp_compact.rs::ensure_english (manténlos en sync).
PROMPT_TMPL = (
    "Translate the following Spanish note into clear, faithful English. "
    "Preserve EVERY fact, name, number, path and identifier exactly as-is. "
    "Be concise but omit nothing. Output ONLY the English translation, with no "
    "preamble, quotes or notes.\n\n{body}"
)

# Recuerdos reales estilo AION tal como se almacenan. AION lo usan tanto hispanohablantes
# como italianos (Ariel vive en Italia): el ahorro en Claude Code debe darse en AMBAS lenguas,
# así que se verifican las dos. El gate del backend (needs_english_translation) detecta ambas.
SAMPLES = {
    "ES": [
        "Ariel decidió usar Rust para el núcleo de AION porque la seguridad de memoria y el rendimiento sin recolector de basura son críticos para un agente local que corre todo el día.",
        "Cuando el agente entra en bucle de 8 vueltas y da timeout, suele ser por descripciones de herramientas recortadas que rompen las llamadas; revertir el recorte lo arregló.",
        "El pendiente crítico no es el grafo sino la autenticación y el CORS de la API local en el puerto 8765, según la auditoría integral de junio de 2026.",
    ],
    "IT": [
        "Ariel ha deciso di usare Rust per il nucleo di AION perché la sicurezza della memoria e le prestazioni senza garbage collector sono critiche per un agente locale che gira tutto il giorno.",
        "Quando l'agente entra in un ciclo di 8 giri e va in timeout, di solito è per le descrizioni degli strumenti troncate che rompono le chiamate; ripristinare il taglio lo ha risolto.",
        "Il punto critico non è il grafo ma l'autenticazione e il CORS dell'API locale sulla porta 8765, secondo l'audit completo di giugno 2026.",
    ],
}


def ollama_up() -> bool:
    try:
        urllib.request.urlopen("http://127.0.0.1:11434/api/tags", timeout=2)
        return True
    except Exception:
        return False


def translate(body: str) -> str:
    req = urllib.request.Request(
        OLLAMA,
        data=json.dumps(
            {
                "model": MODEL,
                "prompt": PROMPT_TMPL.format(body=body),
                "stream": False,
                # gemma4-reason es un modelo de RAZONAMIENTO: sin think:false gasta todo
                # el presupuesto "pensando" y `response` vuelve vacío. mcp_compact.rs envía
                # think:false vía OllamaEngine; aquí debemos replicarlo para medir igual.
                "think": False,
                "options": {"temperature": 0.1, "num_predict": 220},
            }
        ).encode(),
        headers={"Content-Type": "application/json"},
    )
    with urllib.request.urlopen(req, timeout=120) as r:
        return json.loads(r.read())["response"].strip()


def main() -> int:
    if not ollama_up():
        print("❌ Ollama no responde en :11434. Arranca AION/Ollama y reintenta.")
        return 1
    try:
        import tiktoken

        enc = tiktoken.get_encoding("cl100k_base")
        tok = lambda s: len(enc.encode(s))
    except ImportError:
        print("⚠️  tiktoken no instalado; mido por caracteres (proxy más burdo).")
        tok = len

    grand_src = grand_en = 0
    print(f"\n{'='*72}\nVERIFICACIÓN mcp_compact — traducción Gemma local + ahorro real (ES + IT)\n{'='*72}")
    for lang, samples in SAMPLES.items():
        sub_src = sub_en = 0
        for i, src in enumerate(samples, 1):
            en = translate(src)
            ts, tn = tok(src), tok(en)
            sub_src += ts
            sub_en += tn
            print(f"\n— {lang} · recuerdo {i} —")
            print(f"  {lang} ({ts} tok): {src}")
            print(f"  EN ({tn} tok): {en}")
            print(f"  ahorro: {1 - tn/max(ts,1):.0%}")
        grand_src += sub_src
        grand_en += sub_en
        print(f"\n  ▸ subtotal {lang}: {sub_src}→{sub_en} tok  →  ahorro {1 - sub_en/max(sub_src,1):.0%}")
    print(f"\n{'-'*72}")
    print(f"TOTAL  origen={grand_src} tok  EN={grand_en} tok  →  ahorro real: {1 - grand_en/max(grand_src,1):.0%}")
    print("Revisa arriba que la traducción NO pierda hechos, nombres ni números (ES e IT).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
