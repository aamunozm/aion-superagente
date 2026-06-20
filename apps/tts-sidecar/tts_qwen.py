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
import unicodedata
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


# Instrucción de ESTILO (Qwen3 `instruct`): controla emoción/prosodia en lenguaje natural.
# Por defecto, cálido y conversacional → voz más HUMANA (no plana). Configurable/vacío con
# AION_TTS_INSTRUCT="". Se aplica también al clon (probado: combina con ref_audio sin romperlo).
INSTRUCT = os.environ.get(
    "AION_TTS_INSTRUCT",
    "Tono cálido y cercano, conversación relajada y natural, nunca plano ni robótico.",
).strip()


def _norm(s: str) -> str:
    """minúsculas + sin acentos → matching de léxico robusto en ES/IT/EN."""
    s = unicodedata.normalize("NFD", s.lower())
    return "".join(c for c in s if unicodedata.category(c) != "Mn")


# Léxico emocional MULTI-IDIOMA (español latino · italiano · inglés), normalizado sin
# acentos. Detecta la emoción por CONTENIDO, no solo por signos → prosodia mucho más rica.
_LEX = {
    "empatia": [
        "lo siento", "perdona", "perdoname", "disculpa", "disculpame", "te entiendo",
        "entiendo como", "lamento", "tranquilo", "tranquila", "no te preocupes", "descuida",
        "mi dispiace", "scusa", "scusami", "capisco", "tranquillo", "non preoccuparti",
        "i'm sorry", "im sorry", "i am sorry", "i understand", "no worries", "take it easy",
    ],
    "alegria": [
        "genial", "increible", "me encanta", "que bueno", "que bien", "fantastico",
        "estupendo", "maravilloso", "buenisimo", "me alegro", "que alegria", "alucinante",
        "espectacular", "que emocion", "felicidades", "enhorabuena",
        "che bello", "stupendo", "meraviglioso", "mi piace tanto", "evviva", "fantastica",
        "great", "awesome", "amazing", "i love", "love it", "wonderful", "so happy", "excited",
    ],
    "duda": [
        "mmm", "hmm", "a ver", "dejame pensar", "dejame ver", "no estoy seguro", "no se",
        "supongo", "quiza", "quizas", "tal vez", "puede que", "mas o menos", "no sabria",
        "vediamo", "fammi pensare", "non sono sicuro", "non so", "forse", "credo che", "boh",
        "let me think", "not sure", "i guess", "maybe", "perhaps", "i think so",
    ],
    "enfasis": [
        "claro que", "por supuesto", "sin duda", "desde luego", "exacto", "exactamente",
        "definitivamente", "totalmente", "te lo aseguro", "de verdad que", "sin lugar a dudas",
        "certo che", "senz'altro", "senza dubbio", "esatto", "assolutamente",
        "of course", "definitely", "absolutely", "for sure", "no doubt", "exactly",
    ],
    "saludo": [
        "hola", "buenas", "buenos dias", "buenas tardes", "buenas noches", "hasta luego",
        "nos vemos", "cuidate", "un abrazo", "que tengas", "que descanses",
        "ciao", "salve", "buongiorno", "buonasera", "a presto", "ci vediamo",
        "hello", "hi there", "good morning", "see you", "take care", "see you soon",
    ],
}

# Cada estilo → (instrucción Qwen3 `instruct`, factor de velocidad). Investigación 2026
# (Hume/OpenAI/ElevenLabs): la directiva de estilo funciona mejor PRECISA y CORTA (≤~100
# chars), emoción específica > genérica; el `instruct` lleva la EMOCIÓN y el `speed` lleva
# la TASA (palancas separadas). El factor multiplica la velocidad base del usuario: algo más
# rápido animados, más lento reflexionando/empáticos (validado: alegría/ira+, ternura/calma−).
_STYLES = {
    "empatia": ("Tono suave y empático, cálido y cercano, transmitiendo calma.", 0.95),
    "duda": ("Tono pausado y reflexivo, pensando en voz alta, con micro-pausas.", 0.92),
    "alegria": ("Tono animado y alegre, expresivo y entusiasta, como sonriendo.", 1.07),
    "enfasis": ("Tono seguro y firme, con convicción cálida, marcando lo importante.", 1.0),
    "pregunta": ("Tono cálido y curioso, con interés genuino, entonación de pregunta.", 1.0),
    "saludo": ("Tono cálido y cercano, como saludando a alguien que aprecias.", 1.03),
}


def pick_style(text: str):
    """PROSODIA EMOCIONAL ADAPTATIVA → (instruct, speed_factor). Elige la emoción según el
    CONTENIDO (léxico multi-idioma + signos), para que la voz viva con lo que dice, como un
    humano. Heurística barata sobre el texto que ya escribe el cerebro."""
    if not INSTRUCT:
        return "", 1.0
    t = text.strip()
    n = _norm(t)
    # Orden de prioridad: la emoción más "marcada" gana. Empatía/duda mandan sobre el resto
    # (definen el tono de apertura); luego alegría/énfasis; signos como respaldo.
    if any(k in n for k in _LEX["empatia"]):
        return _STYLES["empatia"]
    if "…" in t or "..." in t or any(k in n for k in _LEX["duda"]):
        return _STYLES["duda"]
    if "¡" in t or t.count("!") >= 1 or any(k in n for k in _LEX["alegria"]):
        return _STYLES["alegria"]
    if any(k in n for k in _LEX["enfasis"]):
        return _STYLES["enfasis"]
    if "¿" in t or t.rstrip().endswith("?"):
        return _STYLES["pregunta"]
    if any(n.startswith(k) for k in _LEX["saludo"]):
        return _STYLES["saludo"]
    return INSTRUCT, 1.0


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
    instr, sfactor = pick_style(text)  # emoción adaptativa según el contenido → voz humana
    if instr:
        kw["instruct"] = instr
    # Velocidad ADAPTATIVA: el factor de la emoción (animado +, reflexivo −) modula la
    # velocidad base del usuario, con clamp para que nunca suene antinatural.
    eff_speed = max(0.8, min(1.25, (speed or 1.0) * sfactor))
    chunks = []
    sr = 24000
    for r in m.generate(
        text=text, lang_code=qwen_lang(lang), speed=eff_speed, verbose=False, **kw
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
        # Calentar en ESTE hilo con una frase LARGA y realista: MLX compila kernels
        # por longitud de secuencia en la 1ª inferencia (coste ~6-7 s). Si calentamos
        # con "Hola." (corto) ese coste se paga en la PRIMERA frase real del usuario.
        # Con una frase de longitud típica lo pagamos aquí, en segundo plano al arrancar.
        WARM = (
            "Hola Ariel, esta es una frase de calentamiento un poco más larga para "
            "preparar la voz en tiempo real y que la primera respuesta ya salga rápida."
        )
        _generate(WARM, "es", DEFAULT_PRESET, 1.0)
        cs = clips()
        if cs:
            _generate(WARM, "es", cs[0], 1.0)  # también el clip clonado (camino real)
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
