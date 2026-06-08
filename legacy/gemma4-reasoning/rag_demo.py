#!/usr/bin/env python3
"""RAG local 100% offline: embeddings (nomic) + recuperación + Gemma 4 razonando.
   Sin dependencias externas (solo stdlib). Funciona contra Ollama en localhost.
"""
import json, math, urllib.request

OLLAMA = "http://localhost:11434"
EMBED_MODEL = "nomic-embed-text"
LLM = "gemma4-reason"

# --- Base de conocimiento de ejemplo (sustituye por tus docs) ---------
DOCS = [
    "ProntoClick es una empresa con sede en Italia fundada por Ariel Marquez, chileno.",
    "La arquitectura recomendada por defecto es un monolito modular antes que microservicios prematuros.",
    "El stack de IA local usa Ollama con el modelo gemma4-reason (Gemma 4 12B abliterated Q6_K).",
    "La velocidad medida de gemma4-reason en un MacBook Pro M2 Max es de aproximadamente 18 tokens por segundo.",
    "Para exponer el modelo como API se usa el endpoint compatible con OpenAI en el puerto 11434.",
    "El modelo de embeddings utilizado para el RAG es nomic-embed-text, que corre localmente.",
]

def _post(path, payload):
    req = urllib.request.Request(OLLAMA+path, data=json.dumps(payload).encode(),
                                 headers={"Content-Type":"application/json"})
    with urllib.request.urlopen(req, timeout=300) as r:
        return json.load(r)

def embed(text):
    return _post("/api/embeddings", {"model":EMBED_MODEL,"prompt":text})["embedding"]

def cosine(a,b):
    dot=sum(x*y for x,y in zip(a,b))
    na=math.sqrt(sum(x*x for x in a)); nb=math.sqrt(sum(y*y for y in b))
    return dot/(na*nb+1e-9)

def retrieve(query, k=3):
    qv = embed(query)
    scored = sorted(((cosine(qv, dv), d) for d,dv in INDEX), reverse=True)
    return [d for _,d in scored[:k]]

def answer(query):
    ctx = retrieve(query)
    prompt = (f"Usa SOLO el siguiente contexto para responder. Si no está, dilo.\n\n"
              f"CONTEXTO:\n" + "\n".join(f"- {c}" for c in ctx) +
              f"\n\nPREGUNTA: {query}")
    res = _post("/api/generate", {"model":LLM,"prompt":prompt,"stream":False,"think":True})
    return ctx, res.get("response","").strip()

if __name__ == "__main__":
    print("Indexando documentos (embeddings locales)...")
    INDEX = [(d, embed(d)) for d in DOCS]
    print(f"OK: {len(INDEX)} documentos indexados.\n")
    for q in ["¿Quién fundó ProntoClick y de dónde es?",
              "¿Qué velocidad tiene el modelo local y en qué hardware?"]:
        ctx, ans = answer(q)
        print("="*70)
        print(f"❓ {q}")
        print(f"📚 recuperados: {len(ctx)} fragmentos")
        print(f"💬 {ans}\n")
