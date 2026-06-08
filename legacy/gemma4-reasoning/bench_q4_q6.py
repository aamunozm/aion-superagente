import json, time, urllib.request
URL="http://localhost:11434/api/generate"
Q4="hf.co/mradermacher/Huihui-gemma-4-12B-it-abliterated-i1-GGUF:i1-Q4_K_M"
Q6="hf.co/mradermacher/Huihui-gemma-4-12B-it-abliterated-i1-GGUF:i1-Q6_K"

PROMPTS=[
    "Explica en 250 palabras qué es la arquitectura hexagonal y sus ventajas.",
    "Escribe una función Python que invierta las palabras de una frase manteniendo el orden de los caracteres dentro de cada palabra.",
    "Resume las diferencias entre PostgreSQL y MongoDB en 4 puntos.",
]

def run(model, prompt):
    body={"model":model,"prompt":prompt,"stream":False,"think":False}
    req=urllib.request.Request(URL,data=json.dumps(body).encode(),headers={"Content-Type":"application/json"})
    with urllib.request.urlopen(req,timeout=300) as r: res=json.load(r)
    return {
        "gen_tps": res.get("eval_count",0)/(res.get("eval_duration",1)/1e9),
        "prompt_tps": res.get("prompt_eval_count",0)/(res.get("prompt_eval_duration",1)/1e9 or 1),
        "load_s": res.get("load_duration",0)/1e9,
        "tokens": res.get("eval_count",0),
    }

def bench(model, name):
    # warmup + medir carga en frío
    cold=run(model,"hola")
    gen=[]; ptps=[]
    for p in PROMPTS:
        m=run(model,p); gen.append(m["gen_tps"]); ptps.append(m["prompt_tps"])
    print(f"\n### {name}")
    print(f"  Carga en RAM (frío): {cold['load_s']:.1f}s")
    print(f"  Generación: {sum(gen)/len(gen):.1f} tok/s (min {min(gen):.1f}, max {max(gen):.1f})")
    print(f"  Proc. prompt: {sum(ptps)/len(ptps):.0f} tok/s")
    return sum(gen)/len(gen)

print("Benchmarking (esto tarda ~3-4 min)...")
g6=bench(Q6,"Q6_K (9.8 GB)")
g4=bench(Q4,"Q4_K_M (7.4 GB)")
print("\n"+"="*60)
print(f"📊 Q4 es {(g4/g6-1)*100:+.0f}% en velocidad de generación vs Q6")
print(f"   Q6: {g6:.1f} tok/s  |  Q4: {g4:.1f} tok/s")
