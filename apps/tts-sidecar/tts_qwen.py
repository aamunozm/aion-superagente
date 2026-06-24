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
import re
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


# ── Normalización pre-TTS: números y símbolos → PALABRAS del idioma ───────────────
# Investigación 2026: si el texto lleva "1.234", "50%", "32°C" o "€20", Qwen3-TTS hace
# CODE-SWITCH a fonética inglesa en esos tokens ("español con acento inglés" puntual).
# Verbalizarlos en el idioma ANTES de sintetizar lo evita. Conversor propio (sin deps,
# portable) para es/it/en, enteros 0..10^12 + decimales dígito a dígito. Robusto: si algo
# falla, se devuelve el texto original (la voz nunca se rompe por esto).
_ONES = {
    "es": ["", "uno", "dos", "tres", "cuatro", "cinco", "seis", "siete", "ocho", "nueve", "diez",
           "once", "doce", "trece", "catorce", "quince", "dieciséis", "diecisiete", "dieciocho",
           "diecinueve", "veinte", "veintiuno", "veintidós", "veintitrés", "veinticuatro",
           "veinticinco", "veintiséis", "veintisiete", "veintiocho", "veintinueve"],
    "it": ["", "uno", "due", "tre", "quattro", "cinque", "sei", "sette", "otto", "nove", "dieci",
           "undici", "dodici", "tredici", "quattordici", "quindici", "sedici", "diciassette",
           "diciotto", "diciannove", "venti", "ventuno", "ventidue", "ventitré", "ventiquattro",
           "venticinque", "ventisei", "ventisette", "ventotto", "ventinove"],
    "en": ["", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
           "eleven", "twelve", "thirteen", "fourteen", "fifteen", "sixteen", "seventeen",
           "eighteen", "nineteen", "twenty", "twenty-one", "twenty-two", "twenty-three",
           "twenty-four", "twenty-five", "twenty-six", "twenty-seven", "twenty-eight", "twenty-nine"],
}
_TENS = {
    "es": ["", "", "", "treinta", "cuarenta", "cincuenta", "sesenta", "setenta", "ochenta", "noventa"],
    "it": ["", "", "", "trenta", "quaranta", "cinquanta", "sessanta", "settanta", "ottanta", "novanta"],
    "en": ["", "", "", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety"],
}
_HUND = {
    "es": ["", "ciento", "doscientos", "trescientos", "cuatrocientos", "quinientos", "seiscientos",
           "setecientos", "ochocientos", "novecientos"],
    "it": ["", "cento", "duecento", "trecento", "quattrocento", "cinquecento", "seicento",
           "settecento", "ottocento", "novecento"],
    "en": ["", "one hundred", "two hundred", "three hundred", "four hundred", "five hundred",
           "six hundred", "seven hundred", "eight hundred", "nine hundred"],
}
_ZERO = {"es": "cero", "it": "zero", "en": "zero"}
_POINT = {"es": "coma", "it": "virgola", "en": "point"}


def _under_1000(n: int, lang: str) -> str:
    if n == 0:
        return ""
    o, t, h = _ONES[lang], _TENS[lang], _HUND[lang]
    out = []
    hundreds, rem = divmod(n, 100)
    if hundreds:
        if lang == "es" and n == 100:
            return "cien"
        out.append(h[hundreds])
    if rem:
        if rem < 30:
            out.append(o[rem])
        else:
            tens, units = divmod(rem, 10)
            if units:
                if lang == "es":
                    out.append(f"{t[tens]} y {o[units]}")
                elif lang == "it":
                    base = t[tens]
                    # elisión italiana: trenta+uno→trentuno, quaranta+otto→quarantotto
                    if o[units] in ("uno", "otto"):
                        base = base[:-1]
                    out.append(base + o[units])
                else:
                    out.append(f"{t[tens]}-{o[units]}")
            else:
                out.append(t[tens])
    return " ".join(w for w in out if w)


def _int_to_words(n: int, lang: str) -> str:
    if n == 0:
        return _ZERO[lang]
    if n < 0:
        neg = {"es": "menos ", "it": "meno ", "en": "minus "}[lang]
        return neg + _int_to_words(-n, lang)
    if n >= 10**12:
        return None  # demasiado grande → mejor dígito a dígito (lo maneja el caller)
    parts = []
    scales = [
        (10**9, {"es": ("mil millones", "mil millones"), "it": ("miliardo", "miliardi"), "en": ("billion", "billion")}),
        (10**6, {"es": ("un millón", "millones"), "it": ("milione", "milioni"), "en": ("million", "million")}),
        (10**3, {"es": ("mil", "mil"), "it": ("mille", "mila"), "en": ("thousand", "thousand")}),
    ]
    rem = n
    for value, names in scales:
        q, rem = divmod(rem, value)
        if not q:
            continue
        sing, plur = names[lang]
        if value == 10**3:
            if lang == "es":
                parts.append("mil" if q == 1 else f"{_under_1000(q, 'es')} mil")
            elif lang == "it":
                parts.append("mille" if q == 1 else f"{_int_to_words(q, 'it')}mila")
            else:
                parts.append(f"{_int_to_words(q, 'en')} thousand")
        elif lang == "es":
            parts.append(sing if q == 1 and value == 10**6 else f"{_int_to_words(q, 'es')} {plur}")
        elif lang == "it":
            parts.append(sing if q == 1 else f"{_int_to_words(q, 'it')} {plur}")
        else:
            parts.append(f"{_int_to_words(q, 'en')} {plur}")
    if rem:
        parts.append(_under_1000(rem, lang))
    return " ".join(p for p in parts if p)


def _digits_to_words(s: str, lang: str) -> str:
    """Cada dígito por separado: '56' → 'cinco seis' (para decimales)."""
    return " ".join(_ONES[lang][int(d)] if d != "0" else _ZERO[lang] for d in s)


def _int_or_digits(s: str, lang: str) -> str:
    """Entero a palabras; si es enorme (>10^12), dígito a dígito."""
    w = _int_to_words(int(s), lang)
    return w if w is not None else _digits_to_words(s, lang)


def _num_token_to_words(tok: str, lang: str) -> str:
    """'1.234,56'(es) | '1,234.56'(en) | '4.2.1'(versión) → palabras. Sep. según idioma."""
    thou, dec = (".", ",") if lang in ("es", "it") else (",", ".")
    # 1) Decimal estándar: un único separador decimal, fracción de dígitos.
    if tok.count(dec) == 1:
        left, _, right = tok.partition(dec)
        if right.isdigit() and left.replace(thou, "").isdigit():
            return f"{_int_or_digits(left.replace(thou, ''), lang)} {_POINT[lang]} {_digits_to_words(right, lang)}"
    # 2) Entero con agrupación de miles VÁLIDA (1 / 12 / 1.234 / 1.234.567).
    if re.fullmatch(rf"\d{{1,3}}(\{thou}\d{{3}})*", tok):
        return _int_or_digits(tok.replace(thou, ""), lang)
    # 3) es/it: un único "." con ≠3 dígitos detrás también es DECIMAL (3.14, 1.5).
    if lang in ("es", "it") and tok.count(".") == 1:
        a, _, b = tok.partition(".")
        if a.isdigit() and b.isdigit():
            return f"{_int_or_digits(a, lang)} {_POINT[lang]} {_digits_to_words(b, lang)}"
    # 4) Versión / multi-separador (4.2.1): cada grupo, unido por "punto".
    parts = re.split(r"[.,]", tok)
    if len(parts) >= 2 and all(p.isdigit() for p in parts):
        join = {"es": " punto ", "it": " punto ", "en": " point "}[lang]
        return join.join(_int_or_digits(p, lang) for p in parts)
    # 5) Sin patrón claro: número simple o se deja igual.
    bare = tok.replace(thou, "").replace(dec, "")
    return _int_or_digits(bare, lang) if bare.isdigit() else tok


# Símbolos pegados a números → palabra del idioma (antes de convertir los números).
# Nota: solo "°" (U+00B0, grado), NO "º" (U+00BA, ordinal masculino "1º"=primero).
_SYM = {
    "es": [("%", " por ciento"), ("€", " euros"), ("$", " dólares"), ("£", " libras"), ("°", " grados")],
    "it": [("%", " per cento"), ("€", " euro"), ("$", " dollari"), ("£", " sterline"), ("°", " gradi")],
    "en": [("%", " percent"), ("€", " euros"), ("$", " dollars"), ("£", " pounds"), ("°", " degrees")],
}


def normalize_for_tts(text: str, lang: str) -> str:
    """Verbaliza números y símbolos en el idioma para evitar code-switch de acento."""
    try:
        lng = "es"
        for k, v in LANG_MAP.items():
            if lang == k or lang == v:
                lng = k if k in _ONES else ("es" if v == "spanish" else "en")
                break
        if lng not in _ONES:
            lng = "es"
        out = text
        # Temperatura: "32°C"/"°F" → "... grados centígrados/Fahrenheit" (antes del ° suelto,
        # si no la C/F queda colgando: "gradosC").
        _TEMP = {
            "es": (" grados centígrados", " grados Fahrenheit"),
            "it": (" gradi centigradi", " gradi Fahrenheit"),
            "en": (" degrees Celsius", " degrees Fahrenheit"),
        }[lng]
        out = re.sub(r"[°º]\s?C\b", _TEMP[0], out)
        out = re.sub(r"[°º]\s?F\b", _TEMP[1], out)
        out = out.replace("℃", _TEMP[0]).replace("℉", _TEMP[1])
        # Moneda con símbolo a la izquierda: "€20"/"$5" → "20 euros"/"5 dólares".
        for sym, word in _SYM[lng]:
            if sym in ("€", "$", "£"):
                out = re.sub(rf"\{sym}\s?(\d[\d.,]*)", lambda m, w=word: m.group(1) + w, out)
        # Resto de símbolos pegados a la derecha del número: "50%","32°".
        for sym, word in _SYM[lng]:
            out = out.replace(sym, word)
        # Signo negativo pegado a un número y NO precedido por letra/dígito (evita "GPT-4"
        # → "GPT menos cuatro" o rangos "3-5"): " -5" → " menos 5".
        minus = {"es": "menos ", "it": "meno ", "en": "minus "}[lng]
        out = re.sub(r"(?<![\w-])-(?=\d)", minus, out)
        # Números (con separadores) → palabras, SOLO si no están pegados a letras (así
        # "Qwen3", "4B", "mp3", "v2" quedan intactos y se leen como nombre, no "tres/cuatro").
        out = re.sub(
            r"(?<![^\W\d_])(?:\d[\d.,]*\d|\d)(?![^\W\d_])",
            lambda m: _num_token_to_words(m.group(0), lng),
            out,
        )
        out = re.sub(r"\s{2,}", " ", out)  # colapsa espacios dobles que dejan los símbolos
        return out
    except Exception:
        return text  # ante cualquier fallo, jamás romper la voz


_model = None
_ready = threading.Event()  # se activa cuando el modelo está cargado y caliente

# Cola de trabajos hacia el ÚNICO hilo trabajador (serializa toda la generación MLX).
_jobs: "queue.Queue[_Job]" = queue.Queue()


class _Job:
    __slots__ = ("text", "lang", "voice", "speed", "ev", "audio", "sr", "err", "stream_q", "cancel")

    def __init__(self, text, lang, voice, speed):
        self.text = text
        self.lang = lang
        self.voice = voice
        self.speed = speed
        self.ev = threading.Event()
        self.audio = None
        self.sr = 24000
        self.err = None
        # Si != None, el worker emite PCM Int16 chunk a chunk a esta cola (streaming en vivo)
        # en vez de devolver el audio completo. La pone el handler de /tts/stream.
        self.stream_q = None
        # El handler la activa si el cliente CORTA (barge-in): el worker deja de generar de
        # inmediato (libera GPU para la siguiente respuesta) y NO se bloquea llenando la cola.
        self.cancel = threading.Event()


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


# Ventana Hann (raised-cosine) para el crossfade entre chunks de streaming, cacheada por
# longitud de solape (se reutiliza en cada unión).
_HANN_CACHE = {}


def _hann_ramp(n: int):
    r = _HANN_CACHE.get(n)
    if r is None:
        r = (0.5 * (1.0 - np.cos(np.linspace(0.0, np.pi, n, dtype=np.float32)))).astype(np.float32)
        _HANN_CACHE[n] = r
    return r


def _join_crossfade(chunks, overlap: int = 256):
    """Une los chunks del streaming con CROSSFADE Hann en cada frontera (overlap-add). Sin
    esto, la concatenación naive deja micro-clics audibles en las uniones (medido: saltos
    ~0.02 vs ~0.01 interno). 256 muestras ≈ 10.7 ms @ 24 kHz: imperceptible y mata el clic."""
    chunks = [c for c in chunks if c is not None and len(c)]
    if not chunks:
        return np.zeros(1, np.float32)
    if len(chunks) == 1:
        return chunks[0]
    out = np.asarray(chunks[0], dtype=np.float32)
    for nxt in chunks[1:]:
        nxt = np.asarray(nxt, dtype=np.float32)
        ov = min(overlap, len(out), len(nxt))
        if ov >= 16:
            fade = _hann_ramp(ov)  # 0→1
            blended = out[-ov:] * (1.0 - fade) + nxt[:ov] * fade
            out = np.concatenate([out[:-ov], blended, nxt[ov:]])
        else:
            out = np.concatenate([out, nxt])
    return out


# ── Género: este modelo CustomVoice (0.6B) CONVIERTE TODA VOZ CLONADA EN FEMENINA — verificado
# por F0 (YIN): Diego/Mateo y hasta la voz REAL masculina de Ariel ("chile") salen a ~210-245 Hz
# (mujer) aunque el clip de referencia sea claramente masculino, y el preset base no lo corrige.
# Los PRESETS nativos sí tienen género correcto (aiden 144 Hz/ryan 162 Hz = hombre; serena 198/
# vivian 231 = mujer). Por eso las voces "con género" se sirven con PRESET DIRECTO (sin clonar):
# garantiza el género y suena natural. Mapa diseñada→preset (2 hombres + 2 mujeres distintos):
DESIGNED_TO_PRESET = {
    "ES-Diego": "aiden", "ES-Mateo": "ryan", "ES-Camila": "vivian", "ES-Valentina": "serena",
    "IT-Marco": "aiden", "IT-Luca": "ryan", "IT-Giulia": "serena", "IT-Sofia": "vivian",
    "EN-James": "aiden", "EN-Ethan": "ryan", "EN-Emma": "serena", "EN-Charlotte": "vivian",
}
# Presets masculinos. El instruct de EMOCIÓN tendía a SUBIR el tono (feminizar) la voz
# masculina (medido: sin instruct Diego/Mateo salían 133-161 Hz hombre; con instruct, 175-195).
# Para voces de hombre: ANCLA de género en el instruct + temperatura baja (menos varianza de
# género run-a-run del modelo 0.6B). No es 100% (el modelo es inestable) pero da la mejor opción.
MALE_PRESETS = {"aiden", "ryan", "uncle_fu"}
MALE_ANCHOR = "Habla con VOZ DE HOMBRE, tono grave y masculino, natural y cercano."


def _voice_overrides(base_voice, instr):
    """Para voces masculinas: ancla de género (sustituye la emoción que feminizaba) + temp baja.
    Devuelve (instruct_efectivo, temperatura)."""
    if base_voice in MALE_PRESETS:
        return MALE_ANCHOR, 0.5
    return instr, 0.8


def _generate(text: str, lang: str, voice: str, speed: float):
    """Genera audio. SOLO la llama el hilo trabajador (MLX no es reentrante)."""
    m = model()
    kw = {}
    if voice in DESIGNED_TO_PRESET:
        # Voz "diseñada": preset directo de género correcto (NO clonar → no se vuelve femenina).
        kw["voice"] = DESIGNED_TO_PRESET[voice]
    elif (ref := clip_path(voice) if voice else None):
        # Clon real subido por el usuario (p. ej. su propia voz). OJO: el modelo lo feminiza.
        kw["ref_audio"] = ref
        rt = ref_text_for(ref)
        if rt:
            kw["ref_text"] = rt
        kw["voice"] = DEFAULT_PRESET
    else:
        kw["voice"] = voice if voice in PRESETS else DEFAULT_PRESET
    instr, sfactor = pick_style(text)  # emoción adaptativa sobre el texto ORIGINAL (signos/léxico)
    instr, gen_temp = _voice_overrides(kw.get("voice"), instr)  # ancla de género en voces de hombre
    if instr:
        kw["instruct"] = instr
    # Velocidad ADAPTATIVA: el factor de la emoción (animado +, reflexivo −) modula la
    # velocidad base del usuario. Clamp AMPLIO [0.5, 1.5] para RESPETAR el slider del usuario
    # (rango 0.6-1.5) — antes [0.8,1.25] capaba la velocidad elegida (el slider no hacía nada
    # por encima de ~1.17). El factor de emoción (0.92-1.07) cabe dentro sin recortar.
    eff_speed = max(0.5, min(1.5, (speed or 1.0) * sfactor))
    # Normaliza números/símbolos a palabras del idioma → evita code-switch a acento inglés.
    say = normalize_for_tts(text, lang)
    # TWO-PHASE EMIT: stream=True hace que el vocoder emita audio mientras aún genera (1er
    # chunk en ~0.1-0.4s en vez de ~2s; total algo más rápido). Los chunks resultantes se
    # unen con CROSSFADE Hann (necesario: el streaming introduce micro-discontinuidades).
    chunks = []
    sr = 24000
    for r in m.generate(
        text=say,
        lang_code=qwen_lang(lang),
        speed=eff_speed,
        temperature=gen_temp,
        verbose=False,
        stream=True,
        streaming_interval=0.4,
        **kw,
    ):
        chunks.append(np.asarray(r.audio, dtype=np.float32).reshape(-1))
        sr = getattr(r, "sample_rate", None) or getattr(r, "sr", None) or sr
    audio = _join_crossfade(chunks)
    return audio, int(sr)


def _kw_for(voice: str):
    """Construye los kwargs de voz/clon para m.generate (común a normal y streaming)."""
    kw = {}
    if voice in DESIGNED_TO_PRESET:
        kw["voice"] = DESIGNED_TO_PRESET[voice]  # preset directo de género correcto (no clonar)
    elif (ref := clip_path(voice) if voice else None):
        kw["ref_audio"] = ref
        rt = ref_text_for(ref)
        if rt:
            kw["ref_text"] = rt
        kw["voice"] = DEFAULT_PRESET
    else:
        kw["voice"] = voice if voice in PRESETS else DEFAULT_PRESET
    return kw


def _generate_stream_into(job: "_Job"):
    """STREAMING: emite PCM Int16 (24 kHz mono) chunk a chunk a job.stream_q según los va
    produciendo el vocoder (1er audio en ~0.1-0.4s). SOLO la llama el hilo trabajador."""
    m = model()
    kw = _kw_for(job.voice)
    instr, sfactor = pick_style(job.text)
    instr, gen_temp = _voice_overrides(kw.get("voice"), instr)  # ancla de género en voces de hombre
    if instr:
        kw["instruct"] = instr
    eff_speed = max(0.5, min(1.5, (job.speed or 1.0) * sfactor))  # respeta el slider (ver _generate)
    say = normalize_for_tts(job.text, job.lang)
    for r in m.generate(
        text=say,
        lang_code=qwen_lang(job.lang),
        speed=eff_speed,
        temperature=gen_temp,
        verbose=False,
        stream=True,
        streaming_interval=0.3,
        **kw,
    ):
        if job.cancel.is_set():
            break  # barge-in: deja de generar (libera la GPU para la próxima respuesta)
        a = np.asarray(r.audio, dtype=np.float32).reshape(-1)
        if not len(a):
            continue
        pcm = _pcm16(a)
        # put con timeout + recheck de cancel: si el cliente cortó y la cola está llena, NO
        # bloquea el hilo trabajador (que es ÚNICO: bloquearlo colgaría todo el TTS).
        while not job.cancel.is_set():
            try:
                job.stream_q.put(pcm, timeout=0.2)
                break
            except queue.Full:
                continue
    try:
        job.stream_q.put_nowait(None)  # fin del stream (si nadie lee, da igual)
    except queue.Full:
        pass


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
            if job.stream_q is not None:
                _generate_stream_into(job)  # emite PCM a la cola; termina con None
            else:
                job.audio, job.sr = _generate(job.text, job.lang, job.voice, job.speed)
        except Exception as e:  # noqa: BLE001
            job.err = str(e)
            if job.stream_q is not None:
                job.stream_q.put(None)  # cierra el stream aunque falle
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


def synth_stream(text: str, lang: str, voice: str, speed: float):
    """Generador: encola un trabajo de STREAMING y va devolviendo bloques de PCM Int16 según
    el worker los produce. Permite empezar a reproducir en ~0.1-0.4s en vez de esperar todo.
    Al cerrarse (el handler corta por barge-in) activa job.cancel → el worker para de generar."""
    job = _Job(text, lang, voice, speed)
    job.stream_q = queue.Queue(maxsize=64)  # backpressure: el worker no corre muy por delante
    _jobs.put(job)
    try:
        while True:
            item = job.stream_q.get()
            if item is None:
                break
            yield item
    finally:
        job.cancel.set()  # dejaron de leer (cliente cortó / fin) → no sigas generando


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
            # STREAMING: /tts/stream → PCM Int16 24 kHz mono, chunk a chunk (1er audio ~0.1s).
            # El cliente lo reproduce con Web Audio. HTTP/1.0 → cierra al EOF, sin Content-Length.
            if self.path.startswith("/tts/stream"):
                self.send_response(200)
                self.send_header("Content-Type", "application/octet-stream")
                self.send_header("X-AION-TTS-Engine", "qwen")
                self.send_header("X-Sample-Rate", "24000")
                self.send_header("Cache-Control", "no-store")
                self.send_header("Connection", "close")
                self._cors()
                self.end_headers()
                gen = synth_stream(text, lang, voice, speed)
                try:
                    for pcm in gen:
                        self.wfile.write(pcm)
                        self.wfile.flush()
                except (BrokenPipeError, ConnectionResetError):
                    pass  # el cliente cortó (barge-in / cambió de tema): normal
                finally:
                    gen.close()  # dispara el finally del generador → job.cancel.set()
                return
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
