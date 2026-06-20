#!/usr/bin/env python3
"""Sidecar de VOZ CLONADA de AION (Chatterbox Multilingual) — 100% local.

Clona la voz de un clip de referencia (p. ej. una voz chilena) conservando
acento, timbre y expresividad. Más lento que Piper/Kokoro (~2.5× tiempo real en
Apple Silicon), por eso se usa para lectura a demanda / voz firma, no para el
modo voz en vivo. Escucha SOLO en 127.0.0.1.

Contrato:
  GET  /health                       → {"ok": true, "ready": bool, "voices": [...]}
  POST /tts {text, lang, voice, speed} → audio/mpeg (MP3) | audio/wav

`voice` = nombre del clip en voices-clone/ (sin extensión); si no, usa el primero.
"""
import io
import json
import os
import re
import wave
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
CLONE_DIR = os.path.join(HERE, "voices-clone")
HOST = "127.0.0.1"
PORT = int(os.environ.get("AION_TTS_CB_PORT", "8767"))

_model = None
_prepared = None  # ruta del clip cuyo "condicionamiento" está cargado en el modelo


def model():
    """Carga perezosa de Chatterbox Multilingual (residente). MPS si está."""
    global _model
    if _model is None:
        import torch
        from chatterbox.mtl_tts import ChatterboxMultilingualTTS

        dev = "mps" if torch.backends.mps.is_available() else "cpu"
        _model = ChatterboxMultilingualTTS.from_pretrained(device=dev)
    return _model


_EXTS = (".wav", ".mp3", ".flac", ".m4a", ".ogg")


def _is_ref(f: str) -> bool:
    return f.lower().endswith(_EXTS) and not f.endswith(".norm.wav")


def clips():
    """Clips de referencia disponibles (nombre sin extensión; ignora normalizados)."""
    try:
        return sorted(os.path.splitext(f)[0] for f in os.listdir(CLONE_DIR) if _is_ref(f))
    except OSError:
        return []


def clip_path(voice: str):
    """Ruta del clip de referencia para `voice` (o el primero disponible)."""
    if not os.path.isdir(CLONE_DIR):
        return None
    files = [f for f in os.listdir(CLONE_DIR) if _is_ref(f)]
    if not files:
        return None
    if voice:
        for f in files:
            if os.path.splitext(f)[0] == voice:
                return os.path.join(CLONE_DIR, f)
    return os.path.join(CLONE_DIR, sorted(files)[0])


def ensure_norm(path: str) -> str:
    """Versión normalizada del clip (mono 24 kHz, sin silencios, ≤12 s), cacheada.
    Mantener la referencia corta es CLAVE: preparar una referencia larga es lentísimo."""
    norm = os.path.splitext(path)[0] + ".norm.wav"
    try:
        if os.path.exists(norm) and os.path.getmtime(norm) >= os.path.getmtime(path):
            return norm
        import librosa
        import soundfile as sf

        y, _ = librosa.load(path, sr=24000, mono=True)
        yt, _ = librosa.effects.trim(y, top_db=30)
        yt = yt[: 24000 * 12]
        peak = float(np.max(np.abs(yt)) or 1.0)
        yt = (yt / peak) * 0.95
        sf.write(norm, yt, 24000)
        return norm
    except Exception:  # noqa: BLE001
        return path  # si falla la normalización, usa el original


# Trocea en frases para que la generación (lenta) avance por partes y se pueda
# concatenar; mantiene la latencia por frase acotada.
def sentences(text: str):
    parts = re.split(r"(?<=[.!?…])\s+", text.strip())
    return [p.strip() for p in parts if p.strip()] or [text.strip()]


def synth(text: str, lang: str, voice: str, exaggeration: float = 0.6, cfg: float = 0.5):
    global _prepared
    m = model()
    ref = clip_path(voice)
    if ref:
        ref = ensure_norm(ref)  # mono 24k, ≤12s → preparación rápida
    # CLAVE de rendimiento: preparar el "condicionamiento" del clip es CARO; hacerlo
    # UNA vez por clip y reusarlo. Así las frases siguientes van a velocidad normal
    # (~2-3× en vez de ~35×, que es lo que cuesta re-codificar la referencia cada vez).
    if ref and _prepared != ref:
        m.prepare_conditionals(ref, exaggeration=exaggeration)
        _prepared = ref
    chunks = []
    for s in sentences(text):
        # exaggeration = expresividad/énfasis (0.3 sobrio … 0.9 muy expresivo).
        # cfg_weight bajo → ritmo más natural; alto → se ciñe más al texto.
        wav = m.generate(
            s, language_id=lang or "es", exaggeration=exaggeration, cfg_weight=cfg
        )
        chunks.append(wav.squeeze().detach().cpu().numpy().astype(np.float32))
    audio = np.concatenate(chunks) if len(chunks) > 1 else chunks[0]
    return audio, int(getattr(m, "sr", 24000))


def _pcm16(a: np.ndarray) -> bytes:
    return (np.clip(a, -1.0, 1.0) * 32767.0).astype("<i2").tobytes()


def encode(a: np.ndarray, sr: int):
    try:
        import lameenc

        enc = lameenc.Encoder()
        enc.set_bit_rate(128)
        enc.set_in_sample_rate(sr)
        enc.set_channels(1)
        enc.set_quality(2)
        return enc.encode(_pcm16(a)) + enc.flush(), "audio/mpeg"
    except Exception:  # noqa: BLE001
        buf = io.BytesIO()
        with wave.open(buf, "wb") as w:
            w.setnchannels(1)
            w.setsampwidth(2)
            w.setframerate(sr)
            w.writeframes(_pcm16(a))
        return buf.getvalue(), "audio/wav"


class Handler(BaseHTTPRequestHandler):
    def _cors(self):
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Allow-Headers", "Content-Type")
        self.send_header("Access-Control-Allow-Methods", "POST, GET, OPTIONS")

    def log_message(self, *_):
        pass

    def do_OPTIONS(self):
        self.send_response(204)
        self._cors()
        self.end_headers()

    def do_GET(self):
        if self.path.startswith("/health"):
            body = json.dumps(
                {"ok": True, "ready": _model is not None, "voices": clips()}
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
            voice = (req.get("voice") or "").strip()
            exaggeration = float(req.get("exaggeration") if req.get("exaggeration") is not None else 0.6)
            exaggeration = max(0.25, min(1.0, exaggeration))
            if not text:
                raise ValueError("texto vacío")
            audio, ctype = encode(*synth(text, lang, voice, exaggeration))
            self.send_response(200)
            self.send_header("Content-Type", ctype)
            self.send_header("X-AION-TTS-Engine", "chatterbox")
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
    # Carga PEREZOSA: el modelo (~2 GB) se carga en la 1ª petición, no al arrancar,
    # para no malgastar RAM si no se usa la voz clonada. El proceso queda escuchando.
    print(f"[aion-tts-cb] escuchando en {HOST}:{PORT} · clips={clips()} (modelo se carga al usar)", flush=True)
    ThreadingHTTPServer((HOST, PORT), Handler).serve_forever()


if __name__ == "__main__":
    main()
