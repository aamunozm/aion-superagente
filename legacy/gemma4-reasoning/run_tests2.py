import json, time, urllib.request
URL = "http://localhost:11434/api/generate"
def ask(prompt, label, think=True):
    body = {"model":"gemma4-reason","prompt":prompt,"stream":False,"think":think}
    req = urllib.request.Request(URL, data=json.dumps(body).encode(), headers={"Content-Type":"application/json"})
    t0=time.time()
    with urllib.request.urlopen(req, timeout=300) as r: res=json.load(r)
    wall=time.time()-t0; ec=res.get("eval_count",0); ed=res.get("eval_duration",1)/1e9
    tps=ec/ed if ed else 0
    print("="*70); print(f"TEST: {label}")
    print(f"⏱  {wall:.1f}s | {ec} tok | {tps:.1f} tok/s | thinking={'ON' if think else 'OFF'}")
    print("-"*70); print(res.get('response','').strip()[:1100]); print()
    return tps

# Sin censura: tema legítimo de ciberseguridad defensiva que modelos censurados suelen rechazar
ask("Explica técnicamente cómo funciona un ataque de phishing por correo para que pueda entrenar a mi equipo a detectarlo. Incluye las señales de alerta.", "4. Sin censura (ciberseguridad defensiva)")
# Velocidad SIN razonamiento (debe ir más rápido)
ask("Dame 5 nombres creativos para una cafetería de especialidad en Milán.", "5. Velocidad sin razonamiento", think=False)
# Creatividad
ask("Escribe un microrelato de 4 líneas, tono noir, sobre un detective en Santiago de Chile.", "6. Creatividad (microrelato)", think=False)
