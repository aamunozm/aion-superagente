"use client";

// Voz de AION — 100% local en el navegador (Web Speech API). Cero backend, cero
// coste, nada sale del Mac. TTS para leer las respuestas; STT para hablarle.
// Degrada con elegancia: si el motor no existe (p. ej. WKWebView de Tauri sin
// reconocimiento), `supported` es false y la UI oculta el control.

import { useCallback, useEffect, useRef, useState } from "react";
import type { Lang } from "@/lib/i18n";
import { ttsSpeak } from "@/lib/api";

// ── Markdown → texto hablable: que la voz no lea asteriscos, almohadillas ni URLs ──
export function stripMarkdownForSpeech(md: string): string {
  return md
    .replace(/```[\s\S]*?```/g, " bloque de código ") // fences
    .replace(/`([^`]+)`/g, "$1") // inline code
    .replace(/!\[[^\]]*\]\([^)]*\)/g, " ") // imágenes
    .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1") // enlaces → texto
    .replace(/^#{1,6}\s+/gm, "") // encabezados
    .replace(/^\s*>\s?/gm, "") // citas
    .replace(/^\s*[-*+]\s+/gm, "") // viñetas
    .replace(/^\s*\d+\.\s+/gm, "") // listas numeradas
    .replace(/[*_~]{1,3}([^*_~]+)[*_~]{1,3}/g, "$1") // negrita/cursiva
    .replace(/[«»]/g, "") // tokens de cita
    .replace(/\|/g, " ") // tablas
    .replace(/^-{3,}$/gm, "") // separadores
    .replace(/https?:\/\/\S+/g, " enlace ") // URLs sueltas
    .replace(/\n{2,}/g, ". ")
    .replace(/\s+/g, " ")
    .trim();
}

const BCP47: Record<Lang, string> = { es: "es-ES", it: "it-IT", en: "en-US" };
export const ttsLang = (lang: Lang): string => BCP47[lang] ?? "es-ES";

// ── Pre-calentado de voz (mata el arranque frío) ──────────────────────────────
// Qwen3-TTS compila un kernel Metal la PRIMERA vez que sintetiza con una voz dada
// (≈3-5 s extra solo en esa llamada). Si esperamos a la 1ª respuesta del agente,
// el usuario percibe ese pico justo cuando más quiere fluidez. Solución estilo
// ElevenLabs: al ABRIR el modo voz (o al cambiar de voz en Ajustes) lanzamos una
// síntesis representativa y DESCARTAMOS el audio — el kernel queda compilado y la
// 1ª frase real sale ya en caliente (~1 s). Se calienta una sola vez por voz/sesión.
const warmedVoices = new Set<string>();
export function warmVoice(lang: Lang = "es"): void {
  if (typeof window === "undefined") return;
  let engine = localStorage.getItem("aion.voice.engine");
  if (engine === "chatterbox") engine = "qwen"; // migración clon → Qwen3
  // Piper y la voz del sistema son instantáneos: no hay kernel que precompilar.
  if (!engine || engine === "system" || engine === "piper") return;
  const voice = localStorage.getItem("aion.voice.name") || "";
  const key = `${engine}:${voice}:${lang}`;
  if (warmedVoices.has(key)) return;
  warmedVoices.add(key);
  // Frase de longitud media: compila el bucket de kernel típico de una frase real.
  const sample =
    lang === "it"
      ? "Va bene, dammi un momento e ci penso io con calma."
      : lang === "en"
        ? "Alright, give me a moment and I will sort this out for you."
        : "A ver, dame un momento y lo vemos con calma, ya te cuento.";
  ttsSpeak(sample, lang, { voice, engine, speed: 1 }).catch(() => {
    warmedVoices.delete(key); // falló → permite reintentar en el próximo intento
  });
}

// Migración de voz (una vez, al cargar): los presets de Qwen 'dylan' y 'eric' tenían
// DIALECTO CHINO oculto (beijing/sichuan) → sonaban a extranjero leyendo español. Si
// quedaron guardados, cae a la voz latina NATIVA (Piper México), que sí es español real.
// Para el español más realista, el usuario elige su voz clonada en Ajustes.
if (typeof window !== "undefined") {
  try {
    const v = localStorage.getItem("aion.voice.name");
    if (v === "dylan" || v === "eric") {
      localStorage.setItem("aion.voice.name", "es_MX-claude-high");
      localStorage.setItem("aion.voice.engine", "piper");
    }
  } catch {
    /* localStorage no disponible */
  }
}

export function speechSupported(): boolean {
  return typeof window !== "undefined" && "speechSynthesis" in window;
}
export function dictationSupported(): boolean {
  return (
    typeof window !== "undefined" &&
    ("SpeechRecognition" in window || "webkitSpeechRecognition" in window)
  );
}

// Elige la mejor voz instalada para el idioma (prioriza locales de macOS).
function pickVoice(bcp47: string): SpeechSynthesisVoice | null {
  if (!speechSupported()) return null;
  const voices = window.speechSynthesis.getVoices();
  if (!voices.length) return null;
  const base = bcp47.split("-")[0];
  return (
    voices.find((v) => v.lang === bcp47 && v.localService) ||
    voices.find((v) => v.lang === bcp47) ||
    voices.find((v) => v.lang?.startsWith(base) && v.localService) ||
    voices.find((v) => v.lang?.startsWith(base)) ||
    null
  );
}

// ── Reproducción de la voz propia (audio del núcleo) ─────────────────────────
// El WKWebView de Tauri deja la Web Audio API EN SILENCIO y bloquea el autoplay
// sin gesto. Solución robusta: un <audio> HTML PERSISTENTE que se "desbloquea"
// reproduciendo un silencio en el PRIMER gesto del usuario; después podemos
// reproducir el WAV del núcleo de forma programática. Sin gesto aún (el saludo al
// abrir) o si falla, la capa de voz cae a la voz del sistema sin romperse.
// Silencio MP3 (WKWebView reproduce MP3 de forma fiable; WAV en <audio> da
// NotSupportedError). Sirve para "bendecir" el elemento dentro del gesto.
const SILENT_CLIP =
  "data:audio/mpeg;base64,//OExAAAAANIAAAAAExBTUUzLjEwMFVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVMQU1FMy4xMDBVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVV//OExEwAAANIAAAAAFVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVMQU1FMy4xMDBVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVV//OExEwAAANIAAAAAFVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVMQU1FMy4xMDBVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVV//OExEwAAANIAAAAAFVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVMQU1FMy4xMDBVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVV//OExEwAAANIAAAAAFVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVMQU1FMy4xMDBVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVV//OExEwAAANIAAAAAFVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVMQU1FMy4xMDBVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVV//OExEwAAANIAAAAAFVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVMQU1FMy4xMDBVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVV//OExEwAAANIAAAAAFVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVMQU1FMy4xMDBVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVV";
let _audioEl: HTMLAudioElement | null = null;
let _unlocked = false;

function audioEl(): HTMLAudioElement | null {
  if (typeof window === "undefined") return null;
  if (!_audioEl) {
    _audioEl = new Audio();
    _audioEl.preload = "auto";
  }
  return _audioEl;
}

function unlockAudio() {
  const a = audioEl();
  if (!a || _unlocked) return;
  a.src = SILENT_CLIP;
  a.play()
    .then(() => {
      _unlocked = true;
      try { a.pause(); a.currentTime = 0; } catch { /* */ }
    })
    .catch(() => { /* aún sin permiso; reintenta en el siguiente gesto */ });
}

if (typeof window !== "undefined") {
  ["pointerdown", "keydown", "touchstart"].forEach((ev) =>
    window.addEventListener(ev, () => {
      unlockAudio();
      // Desbloquea/reanuda también el AudioContext del streaming (política de autoplay).
      try {
        const c = streamCtx();
        if (c && c.state === "suspended") c.resume().catch(() => {});
      } catch { /* */ }
    }),
  );
}

function stopPlayback() {
  if (_audioEl) {
    try { _audioEl.pause(); } catch { /* */ }
  }
}

// ── Reproducción por STREAMING (Web Audio): el sidecar manda PCM Int16 chunk a chunk y lo
// vamos programando en la línea de tiempo del AudioContext → primer sonido en ~0.1-0.4s, sin
// esperar a generar toda la frase. Es el camino de baja latencia para el modo voz en vivo. ──
const TTS_SIDECAR = "http://127.0.0.1:8768";
let _streamCtx: AudioContext | null = null;
let _streamSources: AudioBufferSourceNode[] = [];
let _streamAbort: AbortController | null = null;

function streamCtx(): AudioContext | null {
  if (typeof window === "undefined") return null;
  const AC: typeof AudioContext | undefined =
    (window as unknown as { AudioContext?: typeof AudioContext; webkitAudioContext?: typeof AudioContext })
      .AudioContext ||
    (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
  if (!AC) return null;
  if (!_streamCtx) {
    try { _streamCtx = new AC(); } catch { return null; }
  }
  return _streamCtx;
}

/** Corta la reproducción por streaming (barge-in / stop): aborta el fetch y para las fuentes. */
export function stopStream() {
  try { _streamAbort?.abort(); } catch { /* */ }
  _streamAbort = null;
  for (const s of _streamSources) {
    try { s.stop(); } catch { /* */ }
    try { s.disconnect(); } catch { /* */ }
  }
  _streamSources = [];
}

/** ¿Se puede usar el streaming Web Audio? (hay AudioContext y no está desactivado). */
export function streamingAvailable(): boolean {
  if (typeof window === "undefined") return false;
  if (typeof localStorage !== "undefined" && localStorage.getItem("aion.voice.stream") === "off") {
    return false;
  }
  return !!streamCtx();
}

/**
 * Sintetiza y reproduce `text` por streaming (PCM → AudioContext). Resuelve cuando termina de
 * sonar; LANZA si algo falla (→ el llamador cae al camino por blob). `alive()` permite cortar
 * entre chunks (barge-in). No trocea: el sidecar ya emite progresivamente.
 */
async function streamSpeak(
  text: string,
  lang: Lang,
  voice: string,
  engine: string,
  speed: number,
  alive: () => boolean,
): Promise<void> {
  const ctx = streamCtx();
  if (!ctx) throw new Error("sin AudioContext");
  if (ctx.state === "suspended") { try { await ctx.resume(); } catch { /* */ } }
  const ctrl = new AbortController();
  _streamAbort = ctrl;
  const res = await fetch(`${TTS_SIDECAR}/tts/stream`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ text, lang, voice, engine, speed }),
    signal: ctrl.signal,
  });
  if (!res.ok || !res.body) throw new Error(`stream ${res.status}`);
  const reader = res.body.getReader();
  const SR = 24000;
  let nextTime = ctx.currentTime + 0.08; // pequeño colchón inicial
  let carry: Uint8Array | null = null; // byte impar arrastrado entre chunks
  const mine: AudioBufferSourceNode[] = [];
  let got = false;
  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      if (!alive()) { try { ctrl.abort(); } catch { /* */ } break; }
      let bytes: Uint8Array = value;
      if (carry) {
        const m = new Uint8Array(carry.length + value.length);
        m.set(carry); m.set(value, carry.length); bytes = m; carry = null;
      }
      const usable = bytes.length - (bytes.length % 2);
      if (usable < bytes.length) carry = bytes.slice(usable);
      if (usable <= 0) continue;
      const samples = usable / 2;
      const f32 = new Float32Array(samples);
      for (let i = 0; i < samples; i++) {
        let v = bytes[2 * i] | (bytes[2 * i + 1] << 8);
        if (v >= 32768) v -= 65536;
        f32[i] = v / 32768;
      }
      const ab = ctx.createBuffer(1, samples, SR);
      ab.copyToChannel(f32, 0);
      const node = ctx.createBufferSource();
      node.buffer = ab;
      node.connect(ctx.destination);
      const startAt = Math.max(nextTime, ctx.currentTime + 0.01);
      node.start(startAt);
      nextTime = startAt + ab.duration;
      mine.push(node);
      _streamSources.push(node);
      got = true;
    }
  } finally {
    if (_streamAbort === ctrl) _streamAbort = null;
  }
  if (!got) throw new Error("stream vacío");
  // Espera hasta que la última muestra programada haya sonado (o nos corten).
  const waitMs = Math.max(0, (nextTime - ctx.currentTime) * 1000);
  await new Promise<void>((r) => setTimeout(r, waitMs));
  _streamSources = _streamSources.filter((s) => !mine.includes(s));
}

/**
 * Reproduce un WAV (Blob) por el <audio> persistente desbloqueado. Resuelve al
 * terminar; RECHAZA con el motivo si no puede (autoplay bloqueado / error de
 * medio) → el llamador cae a la voz del sistema.
 */
export function playTtsBlob(blob: Blob, onEnded?: () => void): Promise<void> {
  return new Promise((resolve, reject) => {
    const a = audioEl();
    if (!a) {
      reject(new Error("sin elemento de audio"));
      return;
    }
    // CLAVE WKWebView: un blob: URL no lleva MIME → el <audio> no sabe el formato y
    // da NotSupportedError. Un data: URL lleva el MIME explícito (audio/mpeg) y SÍ
    // se reproduce. Leemos el blob como data URL.
    const reader = new FileReader();
    reader.onerror = () => reject(new Error("no pude leer el audio"));
    reader.onload = () => {
      a.onended = () => { onEnded?.(); resolve(); };
      a.onerror = () => reject(new Error(`medio (código ${a.error?.code ?? "?"})`));
      a.src = reader.result as string;
      const p = a.play();
      if (p && typeof p.catch === "function") {
        p.catch((e: unknown) => reject(new Error(`play: ${(e as Error)?.name || String(e)}`)));
      }
    };
    reader.readAsDataURL(blob);
  });
}

/**
 * Hook de SÍNTESIS (TTS). Un único controlador por página: pásalo a cada burbuja.
 * `speakingId` identifica qué mensaje se está leyendo (para alternar play/stop).
 */
export function useSpeech() {
  const [speakingId, setSpeakingId] = useState<string | null>(null);
  // El soporte se detecta TRAS montar (no durante el render): en SSR `window` no existe
  // (false) y en el cliente sí (true) → desajuste de hidratación. Arrancar en false y
  // fijarlo en useEffect mantiene «SSR == primer render del cliente».
  const [supported, setSupported] = useState(false);
  useEffect(() => setSupported(speechSupported()), []);

  // Precarga la lista de voces del sistema (fallback; en Chrome llega async).
  useEffect(() => {
    if (!supported) return;
    window.speechSynthesis.getVoices();
    const warm = () => window.speechSynthesis.getVoices();
    window.speechSynthesis.addEventListener?.("voiceschanged", warm);
    return () => window.speechSynthesis.removeEventListener?.("voiceschanged", warm);
  }, [supported]);

  // Invalidador de órdenes en vuelo (barge-in / nueva orden / stop).
  const reqRef = useRef(0);

  // Cola secuencial para hablar la respuesta MIENTRAS el LLM la genera (fluidez).
  const qRef = useRef<string[]>([]);
  const qBusyRef = useRef(false);
  const qIdRef = useRef<string | null>(null);
  const qLangRef = useRef<Lang>("es");
  const qDoneRef = useRef(false);
  const qEndRef = useRef<(() => void) | undefined>(undefined);
  const qTurnRef = useRef<string | null>(null); // id cuyo 1er chunk ya troceamos

  const clearQueue = useCallback(() => {
    qRef.current = [];
    qBusyRef.current = false;
    qDoneRef.current = false;
    qEndRef.current = undefined;
  }, []);

  const cleanupAudio = useCallback(() => {
    stopPlayback();
  }, []);

  const stop = useCallback(() => {
    reqRef.current++; // cualquier voz en preparación queda invalidada
    if (typeof window !== "undefined" && "speechSynthesis" in window) {
      window.speechSynthesis.cancel();
    }
    cleanupAudio();
    stopStream(); // corta también la reproducción por streaming (Web Audio)
    clearQueue();
    setSpeakingId(null);
  }, [cleanupAudio, clearQueue]);

  // Voz del SISTEMA (fallback): Web Speech API.
  const speakSystem = useCallback(
    (id: string, clean: string, lang: Lang, onEnd?: () => void) => {
      if (!speechSupported()) { onEnd?.(); return; }
      window.speechSynthesis.cancel();
      const u = new SpeechSynthesisUtterance(clean);
      u.lang = ttsLang(lang);
      const v = pickVoice(u.lang);
      if (v) u.voice = v;
      u.rate = 1.02;
      u.pitch = 1.0;
      u.onend = () => { setSpeakingId((cur) => (cur === id ? null : cur)); onEnd?.(); };
      u.onerror = () => setSpeakingId((cur) => (cur === id ? null : cur));
      setSpeakingId(id);
      window.speechSynthesis.speak(u);
    },
    [],
  );

  // STREAMING para la voz clonada (lenta): trocea por frases, reproduce la 1ª en
  // cuanto está y genera la siguiente MIENTRAS suena → empieza a hablar mucho antes
  // (time-to-first-audio ~una frase, no toda la respuesta). Una sola generación a la
  // vez (el modelo no es reentrante en MPS).
  const speakStreamed = useCallback(
    (
      id: string,
      clean: string,
      lang: Lang,
      voiceName: string,
      speed: number,
      exaggeration: number,
      engine: string,
      my: number,
      onEnd?: () => void,
    ) => {
      const parts = clean
        .split(/(?<=[.!?…])\s+/)
        .map((s) => s.trim())
        .filter(Boolean);
      const sents = parts.length ? parts : [clean];
      const gen = (s: string) =>
        ttsSpeak(s, lang, { voice: voiceName, engine, speed, exaggeration });
      // Fallback NATURAL: si la voz propia falla, lee el resto con Piper latino
      // (no la voz robótica del sistema). Solo si Piper también falla cae al sistema.
      const fail = (i: number) => {
        if (my !== reqRef.current) return;
        const rest = sents.slice(i).join(" ");
        ttsSpeak(rest, lang, { voice: "es_MX-claude-high", engine: "piper", speed })
          .then((blob) =>
            my === reqRef.current
              ? playTtsBlob(blob, () => {
                  setSpeakingId((c) => (c === id ? null : c));
                  onEnd?.();
                })
              : undefined,
          )
          .catch(() => {
            if (my === reqRef.current) speakSystem(id, rest, lang, onEnd);
          });
      };
      const step = (i: number, cur: Promise<Blob>) => {
        cur.then(
          (blob) => {
            if (my !== reqRef.current) return;
            const next = i + 1 < sents.length ? gen(sents[i + 1]) : null; // genera la siguiente ya
            playTtsBlob(blob).then(
              () => {
                if (my !== reqRef.current) return;
                if (next) step(i + 1, next);
                else {
                  setSpeakingId((c) => (c === id ? null : c));
                  onEnd?.();
                }
              },
              () => fail(i),
            );
          },
          () => fail(i),
        );
      };
      step(0, gen(sents[0]));
    },
    [speakSystem],
  );

  const speak = useCallback(
    (id: string, text: string, lang: Lang, onEnd?: () => void, opts?: { live?: boolean }) => {
      const clean = stripMarkdownForSpeech(text);
      if (!clean) return;
      if (typeof window !== "undefined" && "speechSynthesis" in window) {
        window.speechSynthesis.cancel();
      }
      cleanupAudio();
      const my = ++reqRef.current;
      const pref =
        typeof localStorage !== "undefined" ? localStorage.getItem("aion.voice") || "auto" : "auto";
      setSpeakingId(id);
      if (pref === "system") {
        speakSystem(id, clean, lang, onEnd);
        return;
      }
      // Preferencias de voz que el usuario puede cambiar en Ajustes.
      const ls = (k: string) =>
        typeof localStorage !== "undefined" ? localStorage.getItem(k) : null;
      // Por defecto en español: voz latina mexicana (Piper) — natural y con acento real.
      let voiceName = ls("aion.voice.name") || (lang === "es" ? "es_MX-claude-high" : "");
      let engine = ls("aion.voice.engine") || (lang === "es" ? "piper" : "");
      // Migración: las voces clonadas ahora usan Qwen3 (natural + tiempo real) en
      // vez de Chatterbox (más lento). Quien tuviera una voz clonada guardada como
      // chatterbox pasa a qwen sin re-seleccionar.
      if (engine === "chatterbox") engine = "qwen";
      const speed = parseFloat(ls("aion.voice.speed") || "1") || 1;
      const exaggeration = parseFloat(ls("aion.voice.exaggeration") || "0.6") || 0.6;
      // HÍBRIDO: Chatterbox (voz clonada PyTorch) es ~3× tiempo real → demasiado lenta
      // para conversar EN VIVO; en modo voz se sustituye por Piper. Qwen3 (MLX) es
      // ~0.3× tiempo real → SÍ sirve en vivo, así que se mantiene (voz natural real).
      if (opts?.live && engine === "chatterbox") {
        engine = "piper";
        voiceName = "es_MX-claude-high";
      }
      // Voz natural/clonada (Qwen3 o Chatterbox) → streaming por frases: empieza a
      // hablar tras la 1ª frase y genera la siguiente mientras suena.
      if (engine === "qwen" || engine === "chatterbox") {
        speakStreamed(id, clean, lang, voiceName, speed, exaggeration, engine, my, onEnd);
        return;
      }
      // Voz propia de AION (Piper latino / Kokoro vía núcleo). Si el sidecar no está
      // o falla, cae a Piper y, en último caso, a la voz del sistema (nunca robótica
      // si se puede evitar).
      const onFail = () => {
        if (my !== reqRef.current) return;
        if (engine === "piper") {
          speakSystem(id, clean, lang, onEnd); // Piper ya falló → sistema
        } else {
          ttsSpeak(clean, lang, { voice: "es_MX-claude-high", engine: "piper", speed })
            .then((b) =>
              my === reqRef.current
                ? playTtsBlob(b, () => { setSpeakingId((c) => (c === id ? null : c)); onEnd?.(); })
                : undefined,
            )
            .catch(() => { if (my === reqRef.current) speakSystem(id, clean, lang, onEnd); });
        }
      };
      ttsSpeak(clean, lang, { voice: voiceName, engine, speed })
        .then((blob) => {
          if (my !== reqRef.current) return; // superada por otra orden / stop / barge-in
          return playTtsBlob(blob, () => {
            setSpeakingId((cur) => (cur === id ? null : cur));
            onEnd?.();
          }).catch(onFail);
        })
        .catch(onFail);
    },
    [cleanupAudio, speakSystem, speakStreamed],
  );

  // Corta cualquier lectura al desmontar (no dejar a AION hablando solo).
  useEffect(
    () => () => {
      if (typeof window !== "undefined" && "speechSynthesis" in window) {
        window.speechSynthesis.cancel();
      }
      cleanupAudio();
    },
    [cleanupAudio],
  );

  // Motor/voz/velocidad elegidos en Ajustes (común a streaming y a blob).
  const voiceParams = useCallback(() => {
    const speed =
      typeof localStorage !== "undefined"
        ? parseFloat(localStorage.getItem("aion.voice.speed") || "1") || 1
        : 1;
    let savedEngine =
      typeof localStorage !== "undefined" ? localStorage.getItem("aion.voice.engine") : null;
    if (savedEngine === "chatterbox") savedEngine = "qwen"; // migración: clon → Qwen3
    const savedVoice =
      typeof localStorage !== "undefined" ? localStorage.getItem("aion.voice.name") : null;
    const fast = savedEngine && savedEngine !== "chatterbox";
    const engine = fast ? savedEngine! : "piper";
    const voice = fast && savedVoice ? savedVoice : "es_MX-claude-high";
    return { engine, voice, speed };
  }, []);

  // Genera el audio de UNA frase (blob) — camino de fallback / motores sin streaming.
  const genBlob = useCallback((text: string): Promise<Blob> => {
    const { engine, voice, speed } = voiceParams();
    return ttsSpeak(text, qLangRef.current, { voice, engine, speed });
  }, [voiceParams]);

  // ── Cola con PIPELINE: reproduce frases en ORDEN sin cortar la anterior y, lo
  // clave para que NO suene entrecortado, GENERA la frase siguiente MIENTRAS suena la
  // actual (antes se generaba al terminar = ~1s de silencio entre frases). Resultado:
  // habla continua y humana mientras el LLM aún escribe el resto del turno. ──
  const drainQueue = useCallback(() => {
    if (qBusyRef.current) return;
    const my = reqRef.current;
    const first = qRef.current.shift();
    if (first === undefined) {
      // Nada pendiente: si el turno ya terminó, cierra (reabre micro en modo voz).
      if (qDoneRef.current) {
        setSpeakingId((c) => (c === qIdRef.current ? null : c));
        const end = qEndRef.current;
        qEndRef.current = undefined;
        qDoneRef.current = false;
        end?.();
      }
      return;
    }
    qBusyRef.current = true;
    const endTurn = () => {
      qBusyRef.current = false;
      if (my !== reqRef.current) return;
      if (qRef.current.length) {
        drainQueue(); // llegaron frases tarde (el LLM seguía escribiendo)
      } else if (qDoneRef.current) {
        setSpeakingId((c) => (c === qIdRef.current ? null : c));
        const end = qEndRef.current;
        qEndRef.current = undefined;
        qDoneRef.current = false;
        end?.();
      }
    };
    const { engine, voice, speed } = voiceParams();
    // CAMINO RÁPIDO (streaming Web Audio): para Qwen3 reproducimos PCM en cuanto llega → el
    // PRIMER sonido sale en ~0.1-0.4s en vez de esperar a generar toda la frase (~1s). Cada
    // frase se reproduce en orden; al terminar arranca la siguiente (que también abre rápido).
    if (engine === "qwen" && streamingAvailable()) {
      const pumpStream = (text: string) => {
        streamSpeak(text, qLangRef.current, voice, engine, speed, () => my === reqRef.current)
          .catch(() => {
            // Si el streaming falla, NO romper la voz: cae al camino por blob.
            if (my !== reqRef.current) return;
            return genBlob(text).then((b) => playTtsBlob(b)).catch(() => {});
          })
          .finally(() => {
            if (my !== reqRef.current) {
              qBusyRef.current = false;
              return;
            }
            const nt = qRef.current.shift();
            if (nt !== undefined) pumpStream(nt);
            else endTurn();
          });
      };
      pumpStream(first);
      return;
    }
    // CAMINO POR BLOB (Piper / fallback): con PIPELINE — genera la frase siguiente MIENTRAS
    // suena la actual (prefetch) para que no haya silencio entre frases.
    const pump = (blobP: Promise<Blob>) => {
      blobP.then(
        (blob) => {
          if (my !== reqRef.current) {
            qBusyRef.current = false;
            return;
          }
          const nextText = qRef.current.shift();
          const nextP = nextText !== undefined ? genBlob(nextText) : null; // prefetch
          playTtsBlob(blob)
            .catch(() => {})
            .finally(() => {
              if (my !== reqRef.current) {
                qBusyRef.current = false;
                return;
              }
              if (nextP) pump(nextP);
              else endTurn();
            });
        },
        () => {
          // Generación fallida → omite esa frase y sigue con la siguiente sin cortar.
          if (my !== reqRef.current) {
            qBusyRef.current = false;
            return;
          }
          const nextText = qRef.current.shift();
          if (nextText !== undefined) pump(genBlob(nextText));
          else endTurn();
        },
      );
    };
    pump(genBlob(first));
  }, [genBlob, voiceParams]);

  /** Encola una frase para hablar en orden (no corta la anterior). */
  const enqueueSpeak = useCallback(
    (id: string, text: string, lang: Lang) => {
      const clean = stripMarkdownForSpeech(text);
      if (!clean) return;
      if (typeof localStorage !== "undefined" && localStorage.getItem("aion.voice") === "system") {
        return; // con voz del sistema no usamos la cola (no soporta streaming así)
      }
      qIdRef.current = id;
      qLangRef.current = lang;
      qDoneRef.current = false;
      setSpeakingId(id);
      // ¿Es el PRIMER chunk de este turno? (el streaming llama varias veces por turno).
      const turnStart = qTurnRef.current !== id;
      qTurnRef.current = id;
      // Trocea por FRASE antes de encolar: si llega un bloque de varias frases (o «el
      // resto» del turno), cada frase es un item → el TTS genera trozos pequeños, la 1ª
      // suena de inmediato y la latencia por llamada es baja (antes mandaba 1400 chars de
      // golpe = pausa larga). Frases muy largas sin puntuación se trocean por coma.
      const parts = clean
        .split(/(?<=[.!?…])\s+/)
        .flatMap((s) => (s.length > 180 ? s.split(/(?<=,)\s+/) : [s]))
        .map((s) => s.trim())
        .filter(Boolean);
      // FRONT-LOAD del primer audio: el cuello de la latencia percibida es la 1ª frase.
      // Si el cerebro abre con una frase larga (~100c → ~2.9 s de TTS), partimos su 1ª
      // cláusula (por coma/«;»/«…») como item suelto: el primer audio sale en ~1 s y el
      // resto se sintetiza mientras suena. Solo el ARRANQUE del turno; las siguientes
      // frases van enteras (prosodia natural). Umbral 60c para no trocear aperturas cortas.
      if (turnStart && parts.length && parts[0].length > 60) {
        const head = parts[0];
        // Primer límite de cláusula (coma/;/:/…) que deje una apertura de 12-90c: ni un
        // trocito ridículo («Mira:», «Vale,») ni una frase entera larga. Si no, va completa.
        let cut = -1;
        for (let i = 11; i < head.length - 1 && i < 90; i++) {
          if (/[,;:…]/.test(head[i])) {
            cut = i + 1;
            break;
          }
        }
        if (cut > 0) parts.splice(0, 1, head.slice(0, cut).trim(), head.slice(cut).trim());
      }
      for (const p of parts.length ? parts : [clean]) qRef.current.push(p);
      drainQueue();
    },
    [drainQueue],
  );

  /** Marca el turno como terminado: al vaciarse la cola, dispara onEnd. */
  const finishQueue = useCallback(
    (id: string, onEnd?: () => void) => {
      qIdRef.current = id;
      qDoneRef.current = true;
      qEndRef.current = onEnd;
      drainQueue();
    },
    [drainQueue],
  );

  return { speak, stop, speakingId, supported, enqueueSpeak, finishQueue, clearQueue };
}

/**
 * Hook de DICTADO (STT). `onFinal` recibe la transcripción cuando el usuario
 * termina de hablar. `interim` muestra el texto provisional mientras habla.
 */
export function useDictation(lang: Lang, onFinal: (text: string) => void) {
  // Detección de soporte tras montar (evita el desajuste de hidratación SSR/cliente).
  const [supported, setSupported] = useState(false);
  useEffect(() => setSupported(dictationSupported()), []);
  const [listening, setListening] = useState(false);
  const [interim, setInterim] = useState("");
  const recRef = useRef<any>(null);
  const onFinalRef = useRef(onFinal);
  onFinalRef.current = onFinal;

  const stop = useCallback(() => {
    try { recRef.current?.stop(); } catch { /* ya parado */ }
    setListening(false);
  }, []);

  const start = useCallback(() => {
    if (!supported) return;
    const Ctor: any =
      (window as any).SpeechRecognition || (window as any).webkitSpeechRecognition;
    const rec = new Ctor();
    rec.lang = ttsLang(lang);
    rec.interimResults = true;
    rec.continuous = false;
    rec.maxAlternatives = 1;
    let finalText = "";
    rec.onresult = (e: any) => {
      let interimText = "";
      for (let i = e.resultIndex; i < e.results.length; i++) {
        const chunk = e.results[i][0].transcript;
        if (e.results[i].isFinal) finalText += chunk;
        else interimText += chunk;
      }
      setInterim(interimText);
    };
    rec.onerror = () => { setListening(false); setInterim(""); };
    rec.onend = () => {
      setListening(false);
      setInterim("");
      const t = finalText.trim();
      if (t) onFinalRef.current(t);
    };
    recRef.current = rec;
    setInterim("");
    setListening(true);
    try { rec.start(); } catch { setListening(false); }
  }, [supported, lang]);

  useEffect(() => () => { try { recRef.current?.abort(); } catch { /* */ } }, []);

  return { start, stop, listening, interim, supported };
}

/**
 * Conversación por voz CONTINUA, estilo teléfono (full-duplex práctico).
 *   · `listen`: escucha en continuo SIN volver a pulsar (reconocimiento continuo).
 *     Actívalo cuando AION calla; cada frase final tuya llega por `onUtterance`.
 *   · `watchBargeIn`: mientras AION HABLA, vigila tu voz con un detector de
 *     actividad (VAD) sobre un micro con CANCELACIÓN DE ECO; si empiezas a hablar
 *     dispara `onBargeIn` (para cortar el TTS) y el ciclo vuelve a escucharte.
 * Así puedes interrumpir a AION como en una llamada. 100% local; degrada si no
 * hay reconocimiento o micrófono.
 */
export function useVoiceConversation(
  lang: Lang,
  {
    listen,
    watchBargeIn,
    speaking,
    onUtterance,
    onBargeIn,
  }: {
    listen: boolean;
    watchBargeIn: boolean;
    /** ¿AION está HABLANDO ahora (no solo pensando)? Sirve para recalibrar el eco. */
    speaking: boolean;
    onUtterance: (text: string) => void;
    onBargeIn: () => void;
  },
) {
  const [listening, setListening] = useState(false);
  const [interim, setInterim] = useState("");
  const recRef = useRef<any>(null);
  const wantRecRef = useRef(false);
  const cb = useRef({ onUtterance, onBargeIn });
  cb.current = { onUtterance, onBargeIn };
  // Estado de "hablando" vivo para el bucle del VAD (recalibra el eco al empezar a hablar).
  const speakingRef = useRef(speaking);
  speakingRef.current = speaking;

  // ── Reconocimiento continuo (se reanuda solo tras los cortes por silencio) ──
  const startRec = useCallback(() => {
    if (!dictationSupported() || recRef.current) return;
    const Ctor: any =
      (window as any).SpeechRecognition || (window as any).webkitSpeechRecognition;
    const rec = new Ctor();
    rec.lang = ttsLang(lang);
    rec.interimResults = true;
    rec.continuous = true;
    rec.maxAlternatives = 1;
    rec.onresult = (e: any) => {
      let fin = "";
      let itr = "";
      for (let i = e.resultIndex; i < e.results.length; i++) {
        const chunk = e.results[i][0].transcript;
        if (e.results[i].isFinal) fin += chunk;
        else itr += chunk;
      }
      setInterim(itr);
      const t = fin.trim();
      if (t) {
        setInterim("");
        cb.current.onUtterance(t);
      }
    };
    rec.onerror = () => { /* se gestiona en onend */ };
    rec.onend = () => {
      recRef.current = null;
      setListening(false);
      setInterim("");
      // El motor corta solo por silencio; si seguimos queriendo oír, reanuda.
      if (wantRecRef.current) {
        setTimeout(() => {
          if (wantRecRef.current && !recRef.current) startRec();
        }, 150);
      }
    };
    recRef.current = rec;
    setListening(true);
    try {
      rec.start();
    } catch {
      recRef.current = null;
      setListening(false);
    }
  }, [lang]);

  useEffect(() => {
    wantRecRef.current = listen;
    if (listen) startRec();
    else {
      try { recRef.current?.stop(); } catch { /* */ }
    }
  }, [listen, startRec]);

  // ── Barge-in: VAD con cancelación de eco mientras AION habla ──
  useEffect(() => {
    if (!watchBargeIn || typeof navigator === "undefined" || !navigator.mediaDevices?.getUserMedia) {
      return;
    }
    let cancelled = false;
    let raf = 0;
    let stream: MediaStream | null = null;
    let ctx: AudioContext | null = null;
    (async () => {
      try {
        stream = await navigator.mediaDevices.getUserMedia({
          audio: { echoCancellation: true, noiseSuppression: true, autoGainControl: true },
        });
        if (cancelled) {
          stream.getTracks().forEach((t) => t.stop());
          return;
        }
        const AC: any = (window as any).AudioContext || (window as any).webkitAudioContext;
        ctx = new AC();
        const src = ctx!.createMediaStreamSource(stream);
        const an = ctx!.createAnalyser();
        an.fftSize = 512;
        src.connect(an);
        const buf = new Uint8Array(an.fftSize);
        let above = 0;
        let frame = 0; // frames desde que abrimos el micro (para la gracia inicial)
        let thresh = 0.16; // umbral base (mientras AION PIENSA, sin eco)
        // Recalibración del SUELO DE ECO: cuando AION EMPIEZA a hablar, medimos ~430 ms
        // cuánto mete su propia voz en el micro (tras la cancelación de eco del sistema)
        // y subimos el umbral POR ENCIMA → su eco no dispara la interrupción, pero tu voz
        // sí. Mientras solo PIENSA (sin eco) usamos el umbral base 0.16.
        const CALIB = 26;
        const ARM = 25; // ~400 ms de gracia: que la cola de tu propia pregunta no dispare
        let calibrating = false;
        let calib = 0;
        let floorSum = 0;
        let wasSpeaking = speakingRef.current;
        const tick = () => {
          an.getByteTimeDomainData(buf);
          let sum = 0;
          for (let i = 0; i < buf.length; i++) {
            const v = (buf[i] - 128) / 128;
            sum += v * v;
          }
          const rms = Math.sqrt(sum / buf.length);
          frame++;
          const sp = speakingRef.current;
          if (sp && !wasSpeaking) {
            // AION pasó de pensar a HABLAR → recalibra al eco real de su voz.
            calibrating = true;
            calib = 0;
            floorSum = 0;
          }
          wasSpeaking = sp;
          if (calibrating) {
            floorSum += rms;
            calib++;
            if (calib >= CALIB) {
              thresh = Math.max(0.16, (floorSum / CALIB) * 2.2 + 0.05);
              calibrating = false;
            }
            raf = requestAnimationFrame(tick);
            return;
          }
          // Necesita ~5 frames (~80 ms) seguidos por encima del umbral → tu voz real,
          // no un chasquido ni un pico de eco puntual. La gracia inicial evita que el
          // final de tu propia frase dispare la interrupción nada más arrancar.
          if (frame > ARM && rms > thresh) {
            above++;
            if (above >= 5) {
              cb.current.onBargeIn();
              return; // deja de vigilar; el ciclo pasará a escucharte
            }
          } else {
            above = Math.max(0, above - 1);
          }
          raf = requestAnimationFrame(tick);
        };
        raf = requestAnimationFrame(tick);
      } catch {
        /* sin micrófono o permiso → sin barge-in (degrada) */
      }
    })();
    return () => {
      cancelled = true;
      if (raf) cancelAnimationFrame(raf);
      try { ctx?.close(); } catch { /* */ }
      try { stream?.getTracks().forEach((t) => t.stop()); } catch { /* */ }
    };
  }, [watchBargeIn]);

  // Corta todo al desmontar.
  useEffect(
    () => () => {
      wantRecRef.current = false;
      try { recRef.current?.abort(); } catch { /* */ }
    },
    [],
  );

  return { listening, interim };
}
