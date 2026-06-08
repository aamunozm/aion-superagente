import json, urllib.request
URL="http://localhost:11434/api/generate"
Q4="hf.co/mradermacher/Huihui-gemma-4-12B-it-abliterated-i1-GGUF:i1-Q4_K_M"
Q6="hf.co/mradermacher/Huihui-gemma-4-12B-it-abliterated-i1-GGUF:i1-Q6_K"
# Tareas de precisión donde la cuantización suele notarse
TASKS=[
  ("Cálculo", "Calcula 47 * 89 y luego réstale 1234. Da solo el número final."),
  ("Lógica", "Si todos los gloops son flerps, y algunos flerps son zorps, ¿podemos afirmar que algunos gloops son zorps? Responde SÍ o NO y explica en una frase."),
]
def run(model,prompt):
    body={"model":model,"prompt":prompt,"stream":False,"think":False}
    req=urllib.request.Request(URL,data=json.dumps(body).encode(),headers={"Content-Type":"application/json"})
    with urllib.request.urlopen(req,timeout=300) as r: return json.load(r).get("response","").strip()
for name,t in TASKS:
    print("="*60); print(f"TAREA: {name}  →  {t}")
    print(f"\n  Q6: {run(Q6,t)[:300]}")
    print(f"\n  Q4: {run(Q4,t)[:300]}")
    print()
# Respuestas correctas: 47*89=4183; 4183-1234=2949 | Lógica: NO (no se sigue)
print("✔ Correcto esperado:  Cálculo=2949  |  Lógica=NO")
