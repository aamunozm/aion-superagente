#!/usr/bin/env python3
"""Cliente mínimo para Gemma 4 12B (razonamiento) vía API OpenAI local de Ollama.
   pip install openai
"""
from openai import OpenAI

client = OpenAI(base_url="http://localhost:11434/v1", api_key="ollama")  # api_key es ignorada

resp = client.chat.completions.create(
    model="gemma4-reason",
    messages=[
        {"role": "user", "content": "Tengo 3 servidores y 5 servicios. "
                                    "Diseña cómo distribuirlos para máxima resiliencia. Razona."},
    ],
    temperature=1.0,
    top_p=0.95,
    stream=True,
)

for chunk in resp:
    delta = chunk.choices[0].delta.content or ""
    print(delta, end="", flush=True)
print()
