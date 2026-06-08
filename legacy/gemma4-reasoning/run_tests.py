import json, time, urllib.request

URL = "http://localhost:11434/api/generate"

def ask(prompt, label, think=True):
    body = {
        "model": "gemma4-reason",
        "prompt": prompt,
        "stream": False,
        "think": think,
    }
    data = json.dumps(body).encode()
    req = urllib.request.Request(URL, data=data, headers={"Content-Type":"application/json"})
    t0 = time.time()
    with urllib.request.urlopen(req, timeout=300) as r:
        res = json.load(r)
    wall = time.time() - t0
    ec = res.get("eval_count", 0)
    ed = res.get("eval_duration", 1) / 1e9
    tps = ec / ed if ed else 0
    thinking = res.get("thinking") or ""
    answer = res.get("response","").strip()
    print("="*70)
    print(f"TEST: {label}")
    print(f"⏱  {wall:.1f}s pared | {ec} tokens | {tps:.1f} tok/s")
    if thinking:
        print(f"🧠 [razonamiento: {len(thinking)} chars] -> {thinking[:160].strip()}...")
    print("-"*70)
    print(answer[:1400])
    print()
    return tps

tps_list = []
tps_list.append(ask("Tres interruptores fuera de una habitación controlan tres bombillas dentro. Solo puedes entrar una vez. ¿Cómo sabes qué interruptor controla cada bombilla? Razona.", "1. Razonamiento lógico (acertijo)"))
tps_list.append(ask("Escribe una función Python que detecte si una cadena es un palíndromo ignorando espacios, tildes y mayúsculas. Incluye 2 tests.", "2. Programación"))
tps_list.append(ask("Resume en 3 frases la diferencia entre un monolito modular y microservicios, y di cuándo elegir cada uno.", "3. Conocimiento técnico (español)"))
print("="*70)
if tps_list:
    print(f"📊 VELOCIDAD MEDIA: {sum(tps_list)/len(tps_list):.1f} tok/s en M2 Max (Q6_K)")
