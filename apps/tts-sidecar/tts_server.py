#!/usr/bin/env python3
"""Sidecar TTS local de AION — 100% en el dispositivo, coste cero.

Motores intercambiables tras un mismo contrato HTTP:
  · kokoro     → rápido y natural (onnx), para conversación en vivo.
  · chatterbox → expresivo + voz clonada (se añade en Fase 3; degrada a kokoro).

Carga el modelo UNA vez y queda residente. Escucha SOLO en 127.0.0.1 (local).
Contrato:
  GET  /health                      → {"ok": true, "engines": [...]}
  POST /tts {text, voice, lang, engine, speed} → audio/wav (16-bit PCM)
"""
import io
import json
import os
import sys
import wave
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import numpy as np

TTS_DIR = os.path.dirname(os.path.abspath(__file__))
HOST = "127.0.0.1"
PORT = int(os.environ.get("AION_TTS_PORT", "8766"))

# Voz por defecto por idioma (Kokoro v1.0). Españolas: ef_dora; italianas: if_sara.
DEFAULT_VOICE = {"es": "ef_dora", "it": "if_sara", "en": "af_heart"}

_kokoro = None


def kokoro():
    """Carga perezosa del modelo Kokoro (residente tras la 1ª vez)."""
    global _kokoro
    if _kokoro is None:
        from kokoro_onnx import Kokoro

        _kokoro = Kokoro(
            os.path.join(TTS_DIR, "kokoro-v1.0.onnx"),
            os.path.join(TTS_DIR, "voices-v1.0.bin"),
        )
    return _kokoro


def synth_kokoro(text: str, voice: str, lang: str, speed: float):
    samples, sr = kokoro().create(text, voice=voice, speed=speed, lang=lang)
    return np.asarray(samples, dtype=np.float32), sr


def to_wav_bytes(samples: np.ndarray, sr: int) -> bytes:
    """float32 [-1,1] → WAV PCM16 en memoria."""
    clipped = np.clip(samples, -1.0, 1.0)
    pcm16 = (clipped * 32767.0).astype("<i2")
    buf = io.BytesIO()
    with wave.open(buf, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(sr)
        w.writeframes(pcm16.tobytes())
    return buf.getvalue()


class Handler(BaseHTTPRequestHandler):
    def _cors(self):
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Allow-Headers", "Content-Type")
        self.send_header("Access-Control-Allow-Methods", "POST, GET, OPTIONS")

    def log_message(self, *_):
        pass  # silencioso

    def do_OPTIONS(self):
        self.send_response(204)
        self._cors()
        self.end_headers()

    def do_GET(self):
        if self.path.startswith("/health"):
            body = json.dumps({"ok": True, "engines": ["kokoro"]}).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self._cors()
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        else:
            self.send_response(404)
            self._cors()
            self.end_headers()

    def do_POST(self):
        if not self.path.startswith("/tts"):
            self.send_response(404)
            self._cors()
            self.end_headers()
            return
        try:
            n = int(self.headers.get("Content-Length", "0"))
            req = json.loads(self.rfile.read(n) or b"{}")
            text = (req.get("text") or "").strip()
            lang = (req.get("lang") or "es").strip()
            engine = (req.get("engine") or "kokoro").strip()
            speed = float(req.get("speed") or 1.0)
            voice = (req.get("voice") or DEFAULT_VOICE.get(lang, "ef_dora")).strip()
            if not text:
                raise ValueError("texto vacío")
            # Chatterbox aún no disponible → cae a kokoro (honesto, sin romper).
            samples, sr = synth_kokoro(text, voice, lang, speed)
            wav = to_wav_bytes(samples, sr)
            self.send_response(200)
            self.send_header("Content-Type", "audio/wav")
            self.send_header("X-AION-TTS-Engine", engine if engine == "kokoro" else f"{engine}->kokoro")
            self._cors()
            self.send_header("Content-Length", str(len(wav)))
            self.end_headers()
            self.wfile.write(wav)
        except Exception as e:  # noqa: BLE001
            body = json.dumps({"ok": False, "error": str(e)}).encode()
            self.send_response(500)
            self.send_header("Content-Type", "application/json")
            self._cors()
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)


def main():
    # Precarga el modelo para que la 1ª petición ya sea rápida.
    try:
        kokoro()
        print(f"[aion-tts] kokoro listo · escuchando en {HOST}:{PORT}", flush=True)
    except Exception as e:  # noqa: BLE001
        print(f"[aion-tts] AVISO: no pude precargar kokoro: {e}", file=sys.stderr, flush=True)
    ThreadingHTTPServer((HOST, PORT), Handler).serve_forever()


if __name__ == "__main__":
    main()
