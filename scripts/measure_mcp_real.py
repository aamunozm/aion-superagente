#!/usr/bin/env python3
"""
Medición EXACTA del ahorro de tokens del puente MCP sobre la memoria REAL del usuario.

A diferencia de verify_mcp_compact.py (muestras sintéticas), este lee los recuerdos reales
de ~/Library/Application Support/AION/memory.jsonl, replica EXACTAMENTE lo que sirve el
puente (truncado por consumidor + clave SHA-256 de mcp_compact.rs::key) y compara tokens
ES→EN con tiktoken cl100k. Usa la caché real (mcp_compact_en.json) y traduce con Gemma local
solo lo que falte (mismo prompt que mcp_compact.rs). No escribe nada: solo mide.

Cortes por consumidor (han de coincidir con WARM_PREFIXES en mcp_compact.rs):
  - brief (claude_code.rs)        → 180 chars
  - aion_memory_search (claude_mcp) → 300 chars

Uso:  python3 scripts/measure_mcp_real.py
"""
import hashlib
import json
import sys
import urllib.request
from pathlib import Path

APP = Path.home() / "Library" / "Application Support" / "AION"
MEM = APP / "memory.jsonl"
CACHE = APP / "mcp_compact_en.json"
OLLAMA = "http://127.0.0.1:11434/api/generate"
MODEL = "gemma4-reason:latest"
PROMPT_TMPL = (  # meaning-first, en sync con mcp_compact.rs::ensure_english
    "You are translating a personal-memory note written in Spanish or Italian (it may "
    "contain typos, slang or regional expressions) into English for another AI agent. "
    "First understand what the author MEANS — silently fix obvious typos, interpret idioms "
    "and regionalisms by their intended sense, and resolve ambiguity — then express that "
    "meaning in clear, natural English. Translate the MEANING, not word-for-word. Preserve "
    "EVERY fact, name, number, path and identifier EXACTLY as written; never invent or add "
    "anything that is not in the note. Be concise but omit nothing. Output ONLY the English "
    "translation, with no preamble, quotes or notes.\n\n{body}"
)
# Cortes que aplican brief (180) y aion_memory_search (300) antes de compactar.
CONSUMERS = {"brief (180)": 180, "aion_memory_search (300)": 300}


def key(s: str) -> str:
    """Réplica de mcp_compact.rs::key — SHA-256, primeros 8 bytes en hex (16 chars)."""
    return hashlib.sha256(s.encode()).digest()[:8].hex()


def has_spanish_signal(t: str) -> bool:
    """Réplica laxa de language_detector.rs: ¿vale la pena traducir? (español O italiano).

    Mismo gate que `needs_english_translation`: acentos agudos/ñ/¿¡ (español) o graves
    à è ì ò ù (italiano), o ≥2 palabras función de cualquiera de las dos lenguas.
    """
    if any(c in t for c in "áéíóúÁÉÍÓÚñÑ¡¿àèìòùÀÈÌÒÙ"):
        return True
    fn = {
        # español
        "el", "la", "de", "que", "en", "por", "los", "las", "una", "con",
        "para", "del", "se", "es", "su", "lo", "como", "más", "pero", "sin",
        # italiano
        "il", "lo", "gli", "le", "di", "che", "non", "per", "uno", "un",
        "sono", "anche", "nel", "nella", "alla", "ma", "se", "ed", "e", "della",
    }
    words = [w.strip(".,;:()[]").lower() for w in t.split()]
    return sum(1 for w in words if w in fn) >= 2


def split_tag(s: str):
    s = s.lstrip()
    if s.startswith("[") and "]" in s:
        i = s.index("]")
        return s[: i + 1], s[i + 1:].lstrip()
    return None, s


def translate(body: str) -> str:
    req = urllib.request.Request(
        OLLAMA,
        data=json.dumps({
            "model": MODEL,
            "prompt": PROMPT_TMPL.format(body=body),
            "stream": False,
            "think": False,
            "options": {"temperature": 0.1, "num_predict": 220},
        }).encode(),
        headers={"Content-Type": "application/json"},
    )
    with urllib.request.urlopen(req, timeout=120) as r:
        return json.loads(r.read())["response"].strip()


def ollama_up() -> bool:
    try:
        urllib.request.urlopen("http://127.0.0.1:11434/api/tags", timeout=2)
        return True
    except Exception:
        return False


def main() -> int:
    if not MEM.exists():
        print(f"❌ No existe {MEM}")
        return 1
    cache = json.loads(CACHE.read_text()) if CACHE.exists() else {}
    try:
        import tiktoken
        enc = tiktoken.get_encoding("cl100k_base")
        tok = lambda s: len(enc.encode(s))
        unit = "tok"
    except ImportError:
        print("⚠️  tiktoken no instalado; mido por caracteres (proxy más burdo).")
        tok = len
        unit = "chars"

    # Recuerdos vigentes (no superseded).
    mems = []
    for line in MEM.read_text().splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            r = json.loads(line)
        except json.JSONDecodeError:
            continue
        if not r.get("superseded") and r.get("content", "").strip():
            mems.append(r["content"])

    have_ollama = ollama_up()
    print(f"\n{'='*74}")
    print(f"MEDICIÓN EXACTA — ahorro MCP sobre memoria REAL  ({len(mems)} recuerdos vigentes)")
    print(f"caché: {len(cache)} entradas · Gemma local: {'sí' if have_ollama else 'NO (solo caché)'}")
    print(f"{'='*74}")

    translated_now = 0
    for label, cut in CONSUMERS.items():
        raw = served = 0          # tokens en bruto (ES) vs servidos (EN si hay)
        cached_hits = with_signal = 0
        seen = set()
        for content in mems:
            t = content[:cut]
            if t in seen:          # mismo texto truncado en dos recuerdos: cuenta una vez
                continue
            seen.add(t)
            es_tok = tok(t)
            raw += es_tok
            if has_spanish_signal(t):
                with_signal += 1
            k = key(t)
            en = cache.get(k)
            if en is None and have_ollama and has_spanish_signal(t) and len(t) >= 40:
                tag, body = split_tag(t)
                if len(body) >= 20:
                    try:
                        out = translate(body)
                        if out and len(out) >= len(body) // 5:
                            en = f"{tag} {out}" if tag else out
                            cache[k] = en          # solo en RAM: no se persiste
                            translated_now += 1
                    except Exception:
                        en = None
            if en is not None:
                cached_hits += 1
                served += tok(en)
            else:
                served += es_tok   # fail-open: se sirve español
        saving = (1 - served / raw) if raw else 0.0
        print(f"\n— {label} —")
        print(f"  recuerdos únicos: {len(seen)} · con señal ES: {with_signal} · traducidos: {cached_hits}")
        print(f"  bruto ES: {raw} {unit}  →  servido: {served} {unit}")
        print(f"  AHORRO REAL: {saving:.1%}")

    print(f"\n{'-'*74}")
    print(f"Traducciones nuevas hechas en esta medición (no persistidas): {translated_now}")
    print("Nota: el ahorro escala con la densidad de español; el código y los identificadores")
    print("(ya en inglés) no ahorran. El warmer de arranque deja TODO esto precacheado.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
