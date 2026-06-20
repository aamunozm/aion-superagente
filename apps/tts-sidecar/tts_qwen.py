#!/usr/bin/env python3
"""Sidecar de VOZ NATURAL de AION (Qwen3-TTS vía MLX) — 100% local, tiempo real.

Qwen3-TTS multilingüe corriendo en Apple Silicon con MLX: genera ~3× más rápido
de lo que dura el audio (RTF ~0.3), así que sirve para conversación en vivo y
además **clona** una voz de referencia (p. ej. una voz chilena) con naturalidad
muy superior a Piper/Chatterbox. Escucha SOLO en 127.0.0.1.

⚠️ MLX/Metal NO es seguro entre hilos: generar desde el hilo de cada conexión
cuelga la GPU. Por eso TODA la generación corre en un ÚNICO hilo trabajador
(dueño del modelo); los handlers HTTP solo encolan el trabajo y esperan el
resultado. El warmup también ocurre en ese hilo.

Contrato:
  GET  /health                          → {"ok": true, "ready": bool, "voices":[...], "presets":[...]}
  POST /tts {text, lang, voice, speed}  → audio/mpeg (MP3) | audio/wav

`voice`:
  · uno de los PRESET (serena, vivian, ryan…) → voz Qwen3 nativa de ese hablante.
  · el nombre de un clip en voices-clone/ (sin extensión) → CLONA esa voz.
  · vacío → preset por defecto (serena).
"""
import io
import json
import os
import queue
import threading
import wave
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
CLONE_DIR = os.path.join(HERE, "voices-clone")
HOST = "127.0.0.1"
PORT = int(os.environ.get("AION_TTS_QWEN_PORT", "8768"))

# Modelo PÚBLICO (el oficial Qwen/Qwen3-TTS-* está bloqueado/gated). Esta conversión
# de mlx-community trae CustomVoice (clonación) + hablantes preset, 8-bit (~0.7 GB).
MODEL = os.environ.get(
    "AION_QWEN_MODEL", "mlx-community/Qwen3-TTS-12Hz-0.6B-CustomVoice-8bit"
)
# Hablantes nativos de Qwen3 CustomVoice (multilingües; leen español con su timbre).
PRESETS = [
    "serena", "vivian", "uncle_fu", "ryan", "aiden", "ono_anna", "sohee", "eric", "dylan",
]
DEFAULT_PRESET = "serena"  # femenina, natural en español

# ⚠️ Qwen3 espera el idioma como PALABRA ('spanish'), NO el código ISO ('es'). Pasar
# 'es' hace que fonemice en INGLÉS → "español con acento inglés". Mapear siempre.
LANG_MAP = {
    "es": "spanish", "en": "english", "it": "italian", "pt": "portuguese",
    "fr": "french", "de": "german", "ru": "russian", "ja": "japanese",
    "ko": "korean", "zh": "chinese",
}


def qwen_lang(lang: str) -> str:
    lang = (lang or "es").strip().lower()
    if lang in LANG_MAP:
        return LANG_MAP[lang]
    if lang in LANG_MAP.values():  # ya viene como palabra
        return lang
    return "spanish"  # AION es español-primario; nunca caer a inglés por defecto

_model = None
_ready = threading.Event()  # se activa cuando el modelo está cargado y caliente

# Cola de trabajos hacia el ÚNICO hilo trabajador (serializa toda la generación MLX).
_jobs: "queue.Queue[_Job]" = queue.Queue()


class _Job:
    __slots__ = ("text", "lang", "voice", "speed", "ev", "audio", "sr", "err")

    def __init__(self, text, lang, voice, speed):
        self.text = text
        self.lang = lang
        self.voice = voice
        self.speed = speed
        self.ev = threading.Event()
        self.audio = None
        self.sr = 24000
        self.err = None


def model():
    """Carga del modelo Qwen3-TTS. SOLO la llama el hilo trabajador."""
    global _model
    if _model is None:
        from mlx_audio.tts.utils import load_model

        _model = load_model(MODEL)
    return _model


_EXTS = (".wav", ".mp3", ".flac", ".m4a", ".ogg")


def _is_ref(f: str) -> bool:
    return f.lower().endswith(_EXTS) and not f.endswith(".norm.wav")


def clips():
    """Clips de referencia disponibles para clonar (nombre sin extensión)."""
    try:
        return sorted(os.path.splitext(f)[0] for f in os.listdir(CLONE_DIR) if _is_ref(f))
    except OSError:
        return []


def clip_path(voice: str):
    """Ruta del clip de referencia para `voice` (exacto por nombre, sin extensión)."""
    if not os.path.isdir(CLONE_DIR):
        return None
    for f in os.listdir(CLONE_DIR):
        if _is_ref(f) and os.path.splitext(f)[0] == voice:
            return os.path.join(CLONE_DIR, f)
    return None


def ref_text_for(path: str):
    """Transcripción cacheada del clip (<clip>.txt). Si existe, evita que Qwen3
    re-transcriba la referencia en cada petición (más rápido y estable)."""
    txt = os.path.splitext(path)[0] + ".txt"
    try:
        if os.path.exists(txt):
            t = open(txt, encoding="utf-8").read().strip()
            return t or None
    except OSError:
        pass
    return None


def _generate(text: str, lang: str, voice: str, speed: float):
    """Genera audio. SOLO la llama el hilo trabajador (MLX no es reentrante)."""
    m = model()
    kw = {}
    ref = clip_path(voice) if voice else None
    if ref:
        # Clonar la voz del clip; preset como timbre base que Qwen3 sobrescribe.
        kw["ref_audio"] = ref
        rt = ref_text_for(ref)
        if rt:
            kw["ref_text"] = rt
        kw["voice"] = DEFAULT_PRESET
    else:
        kw["voice"] = voice if voice in PRESETS else DEFAULT_PRESET
    chunks = []
    sr = 24000
    for r in m.generate(
        text=text, lang_code=qwen_lang(lang), speed=(speed or 1.0), verbose=False, **kw
    ):
        chunks.append(np.asarray(r.audio, dtype=np.float32).reshape(-1))
        sr = getattr(r, "sample_rate", None) or getattr(r, "sr", None) or sr
    audio = (
        np.concatenate(chunks)
        if len(chunks) > 1
        else (chunks[0] if chunks else np.zeros(1, np.float32))
    )
    return audio, int(sr)


def _worker():
    """Hilo ÚNICO dueño del modelo: carga, calienta y procesa la cola en serie."""
    try:
        # Calentar en ESTE hilo: preset + (si hay) el primer clip clonado, para que
        # la 1ª petición real ya vaya en caliente (sin cargar modelo/ASR/tokenizer).
        _generate("Hola.", "es", DEFAULT_PRESET, 1.0)
        cs = clips()
        if cs:
            _generate("Hola.", "es", cs[0], 1.0)
        print("[aion-tts-qwen] modelo Qwen3 cargado y caliente", flush=True)
    except Exception as e:  # noqa: BLE001
        print(f"[aion-tts-qwen] AVISO: warmup falló: {e}", flush=True)
    _ready.set()
    while True:
        job = _jobs.get()
        try:
            job.audio, job.sr = _generate(job.text, job.lang, job.voice, job.speed)
        except Exception as e:  # noqa: BLE001
            job.err = str(e)
        finally:
            job.ev.set()


def synth(text: str, lang: str, voice: str, speed: float):
    """Encola el trabajo al hilo trabajador y espera el resultado."""
    job = _Job(text, lang, voice, speed)
    _jobs.put(job)
    job.ev.wait()
    if job.err:
        raise RuntimeError(job.err)
    return job.audio, job.sr


def _pcm16(a: np.ndarray) -> bytes:
    return (np.clip(a, -1.0, 1.0) * 32767.0).astype("<i2").tobytes()


def encode(a: np.ndarray, sr: int):
    """MP3 (WKWebView lo reproduce fiable); si falta lameenc, WAV."""
    try:
        import lameenc

        enc = lameenc.Encoder()
        enc.set_bit_rate(128)
        enc.set_in_sample_rate(int(sr))
        enc.set_channels(1)
        enc.set_quality(2)
        return enc.encode(_pcm16(a)) + enc.flush(), "audio/mpeg"
    except Exception:  # noqa: BLE001
        buf = io.BytesIO()
        with wave.open(buf, "wb") as w:
            w.setnchannels(1)
            w.setsampwidth(2)
            w.setframerate(int(sr))
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
                {"ok": True, "ready": _ready.is_set(), "voices": clips(), "presets": PRESETS}
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
            speed = float(req.get("speed") or 1.0)
            if not text:
                raise ValueError("texto vacío")
            audio, ctype = encode(*synth(text, lang, voice, speed))
            self.send_response(200)
            self.send_header("Content-Type", ctype)
            self.send_header("X-AION-TTS-Engine", "qwen")
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
    print(
        f"[aion-tts-qwen] escuchando en {HOST}:{PORT} · clips={clips()} · model={MODEL}",
        flush=True,
    )
    # Hilo trabajador ÚNICO: carga + warmup + toda la generación (MLX en un solo hilo).
    threading.Thread(target=_worker, daemon=True).start()
    ThreadingHTTPServer((HOST, PORT), Handler).serve_forever()


if __name__ == "__main__":
    main()
