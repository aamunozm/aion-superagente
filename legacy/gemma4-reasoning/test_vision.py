import json, base64, time, urllib.request
URL="http://localhost:11434/api/generate"
with open("test_vision.png","rb") as f:
    img_b64 = base64.b64encode(f.read()).decode()
body={
    "model":"huihui_ai/gemma-4-abliterated:12b",
    "prompt":"Describe esta imagen con detalle: ¿qué formas geométricas y colores ves, cuántas hay, y qué texto aparece?",
    "images":[img_b64],
    "stream":False,
    "think":False,
}
t0=time.time()
req=urllib.request.Request(URL,data=json.dumps(body).encode(),headers={"Content-Type":"application/json"})
with urllib.request.urlopen(req,timeout=300) as r: res=json.load(r)
print(f"⏱ {time.time()-t0:.1f}s")
print("="*70)
print(res.get("response","").strip())
