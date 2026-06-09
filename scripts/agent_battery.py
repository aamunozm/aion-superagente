#!/usr/bin/env python3
"""Batería de pruebas del AGENTE de AION: conciencia, razonamiento, memoria,
herramientas y, sobre todo, SKILLS (auto-escritura). Habla con el núcleo vía HTTP."""
import json, sys, urllib.request

BASE = sys.argv[1] if len(sys.argv) > 1 else "http://127.0.0.1:8765"

def call(path, payload, timeout=180):
    req = urllib.request.Request(
        BASE + path, data=json.dumps(payload).encode(),
        headers={"content-type": "application/json"})
    answer, obs, thoughts = "", [], []
    with urllib.request.urlopen(req, timeout=timeout) as r:
        for raw in r:
            line = raw.decode().strip()
            if not line.startswith("data:"):
                continue
            try:
                d = json.loads(line[5:])
            except Exception:
                continue
            k = d.get("kind")
            if k == "answer": answer += d.get("text", "")
            elif k == "observation": obs.append(d.get("text", ""))
            elif k == "thought": thoughts.append(d.get("text", ""))
    return answer, obs, thoughts

def agent(task): return call("/api/agent", {"task": task})
def chat(prompt): return call("/api/chat", {"prompt": prompt, "think": False})

results = []
def check(name, cond, detail):
    results.append((name, cond))
    print(f"  {'✅' if cond else '❌'} {name}")
    print(f"     {detail[:160]}")

print("══════ BATERÍA DEL AGENTE AION ══════\n")

# 1. CONCIENCIA / autoconciencia
print("◆ Conciencia")
a, *_ = agent("¿Sabes quién eres y dónde estás? Responde directo.")
check("sabe quién es", "aion" in a.lower() and ("mac" in a.lower() or "autónom" in a.lower() or "local" in a.lower()), a)
a2, *_ = chat("¿Qué has estado haciendo mientras no estaba?")
check("habla de su vida (no 'nada')", "nada" not in a2.lower()[:40], a2)

# 2. RAZONAMIENTO + calculadora
print("\n◆ Razonamiento (calculadora)")
a, obs, _ = agent("¿Cuánto es 37*21+8? Usa la calculadora.")
check("razona y calcula (=785)", "785" in (a + " ".join(obs)), a)

# 3. MEMORIA cognitiva (recordar + recuperar)
print("\n◆ Memoria cognitiva")
a, obs, _ = agent("Recuerda este hecho con la herramienta remember: mi color favorito es el verde.")
check("recuerda (escribe en memoria)", "record" in (a + " ".join(obs)).lower(), a)
a, obs, _ = agent("Busca en tu memoria cuál es mi color favorito y dímelo.")
check("recupera de memoria (verde)", "verde" in (a + " ".join(obs)).lower(), a)

# 4. SKILLS — AUTO-ESCRITURA (lo más importante)
print("\n◆ Skills: auto-escritura (skill_forge) + invocación")
a, obs, _ = agent('Usa skill_forge para crear una skill "cubo" (n*n*n) con tests [[2,8],[3,27]]. Luego usa skill_invoke para cubo de 5.')
both = a + " ".join(obs)
check("forja skill 'cubo' en sandbox", "creada" in both.lower() or "validada" in both.lower(), both)
check("invoca la skill forjada (5³=125)", "125" in both, both)

a, obs, _ = agent('Crea con skill_forge una skill "triple" (n*3) con tests [[2,6],[5,15]] y calcula triple de 10.')
both = a + " ".join(obs)
check("forja 2ª skill 'triple' y la usa (30)", "30" in both, both)

# 5. Skill semilla (sum_to)
print("\n◆ Skill semilla (WASM sandbox)")
a, obs, _ = agent("Usa skill_invoke con la skill sum_to y el número 100.")
check("invoca skill semilla sum_to(100)=5050", "5050" in (a + " ".join(obs)), a + " ".join(obs))

# 6. INVESTIGACIÓN (web)
print("\n◆ Investigación (web)")
a, obs, _ = agent("Lee la página https://example.com y dime brevemente de qué trata.")
both = (a + " ".join(obs)).lower()
check("lee la web (navegador propio)", "example" in both or "domain" in both or "dominio" in both, a)

# Resumen
print("\n══════ RESUMEN ══════")
ok = sum(1 for _, c in results if c)
for n, c in results:
    print(f"  {'✅' if c else '❌'} {n}")
print(f"\n  {ok}/{len(results)} pruebas superadas")
sys.exit(0 if ok == len(results) else 1)
