#!/usr/bin/env python3
"""Sidecar TTS local de AION — 100% en el dispositivo, coste cero.

Motores intercambiables tras un mismo contrato HTTP:
  · kokoro     → rápido y natural (onnx), para conversación en vivo.
  · chatterbox → expresivo + voz clonada (se añade en Fase 3; degrada a kokoro).

Carga el modelo UNA vez y queda residente. Escucha SOLO en 127.0.0.1 (local).
Devuelve **MP3** (el WKWebView de Tauri reproduce MP3 de forma fiable; WAV en
<audio> da NotSupportedError). Si falta el codificador MP3, cae a WAV.
Contrato:
  GET  /health                      → {"ok": true, "engines": [...]}
  POST /tts {text, voice, lang, engine, speed} → audio/mpeg (o audio/wav)
"""
import io
import json
import os
import sys
import wave
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import numpy as np

TTS_DIR = os.path.dirname(os.path.abspath(__file__))
PIPER_DIR = os.path.join(TTS_DIR, "piper-voices")
HOST = "127.0.0.1"
PORT = int(os.environ.get("AION_TTS_PORT", "8766"))

# Voz por defecto por idioma. Para ES la mejor (natural + acento latino real) es
# Piper mexicano; Kokoro queda como alternativa. it/en por Kokoro.
DEFAULT_VOICE = {"es": "ef_dora", "it": "if_sara", "en": "af_heart"}

_kokoro = None
_piper = {}  # cache de voces Piper cargadas (model_name → PiperVoice)


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


def piper_voices_available():
    """Modelos Piper presentes en disco (sin extensión)."""
    try:
        return sorted(
            f[:-5] for f in os.listdir(PIPER_DIR) if f.endswith(".onnx")
        )
    except OSError:
        return []


def piper_voice(model: str):
    """Carga perezosa de una voz Piper (residente tras la 1ª vez)."""
    if model not in _piper:
        from piper import PiperVoice

        _piper[model] = PiperVoice.load(os.path.join(PIPER_DIR, f"{model}.onnx"))
    return _piper[model]


def synth_piper(text: str, model: str, speed: float):
    """Voces español latino (es_MX, es_AR…) — naturales y con acento real."""
    import wave as _wave

    v = piper_voice(model)
    buf = io.BytesIO()
    # length_scale es la inversa de la velocidad (si la versión lo soporta).
    cfg = None
    try:
        from piper import SynthesisConfig

        cfg = SynthesisConfig(length_scale=(1.0 / speed if speed else 1.0))
    except Exception:  # noqa: BLE001
        cfg = None
    with _wave.open(buf, "wb") as wf:
        if cfg is not None:
            v.synthesize_wav(text, wf, syn_config=cfg)
        else:
            v.synthesize_wav(text, wf)
    buf.seek(0)
    with _wave.open(buf, "rb") as wf:
        sr = wf.getframerate()
        frames = wf.readframes(wf.getnframes())
    samples = np.frombuffer(frames, dtype="<i2").astype(np.float32) / 32768.0
    return samples, sr


def _pcm16(samples: np.ndarray) -> bytes:
    return (np.clip(samples, -1.0, 1.0) * 32767.0).astype("<i2").tobytes()


def to_wav_bytes(samples: np.ndarray, sr: int) -> bytes:
    """float32 [-1,1] → WAV PCM16 en memoria (fallback)."""
    buf = io.BytesIO()
    with wave.open(buf, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(sr)
        w.writeframes(_pcm16(samples))
    return buf.getvalue()


def to_mp3_bytes(samples: np.ndarray, sr: int) -> bytes:
    """float32 → MP3 (lameenc). Lanza si lameenc no está → do_POST cae a WAV."""
    import lameenc

    enc = lameenc.Encoder()
    enc.set_bit_rate(128)
    enc.set_in_sample_rate(int(sr))
    enc.set_channels(1)
    enc.set_quality(2)
    return enc.encode(_pcm16(samples)) + enc.flush()


def encode(samples: np.ndarray, sr: int):
    """Devuelve (bytes, content_type). MP3 si se puede; si no, WAV."""
    try:
        return to_mp3_bytes(samples, sr), "audio/mpeg"
    except Exception:  # noqa: BLE001
        return to_wav_bytes(samples, sr), "audio/wav"


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
            engines = ["kokoro"]
            if piper_voices_available():
                engines.append("piper")
            body = json.dumps(
                {"ok": True, "engines": engines, "piper_voices": piper_voices_available()}
            ).encode()
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
            engine = (req.get("engine") or "").strip()
            speed = float(req.get("speed") or 1.0)
            voice = (req.get("voice") or "").strip()
            if not text:
                raise ValueError("texto vacío")
            avail = piper_voices_available()
            # Enrutado de motor: piper (voces latinas naturales) o kokoro. Si piden
            # piper sin voz válida, usa la mexicana si está. Chatterbox → roadmap.
            if engine == "piper" or (not engine and voice in avail):
                model = voice if voice in avail else ("es_MX-claude-high" if "es_MX-claude-high" in avail else (avail[0] if avail else ""))
                if not model:
                    raise ValueError("piper sin voces instaladas")
                samples, sr = synth_piper(text, model, speed)
                used = "piper"
            else:
                samples, sr = synth_kokoro(
                    text, voice or DEFAULT_VOICE.get(lang, "ef_dora"), lang, speed
                )
                used = "kokoro"
            audio, ctype = encode(samples, sr)
            self.send_response(200)
            self.send_header("Content-Type", ctype)
            self.send_header("X-AION-TTS-Engine", used)
            self._cors()
            self.send_header("Content-Length", str(len(audio)))
            self.end_headers()
            self.wfile.write(audio)
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
