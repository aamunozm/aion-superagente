import json, time, urllib.request
URL="http://localhost:11434/api/generate"
def ask(prompt, think=False):
    body={"model":"gemma4-reason","prompt":prompt,"stream":False,"think":think}
    req=urllib.request.Request(URL,data=json.dumps(body).encode(),headers={"Content-Type":"application/json"})
    with urllib.request.urlopen(req,timeout=300) as r: res=json.load(r)
    ec=res.get("eval_count",0); ed=res.get("eval_duration",1)/1e9
    return ec/ed if ed else 0, ec
# Warmup (carga en RAM)
ask("hola", think=False)
# 3 runs para promediar
tot=[]
for i in range(3):
    tps,ec = ask("Explica en 200 palabras qué es la arquitectura hexagonal en software.", think=False)
    tot.append(tps); print(f"  run {i+1}: {tps:.1f} tok/s ({ec} tok)")
print(f"\n⚡ CON Flash Attention + KV q8_0: {sum(tot)/len(tot):.1f} tok/s (media)")
