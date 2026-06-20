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
    window.addEventListener(ev, unlockAudio),
  );
}

function stopPlayback() {
  if (_audioEl) {
    try { _audioEl.pause(); } catch { /* */ }
  }
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
      my: number,
      onEnd?: () => void,
    ) => {
      const parts = clean
        .split(/(?<=[.!?…])\s+/)
        .map((s) => s.trim())
        .filter(Boolean);
      const sents = parts.length ? parts : [clean];
      const gen = (s: string) =>
        ttsSpeak(s, lang, { voice: voiceName, engine: "chatterbox", speed, exaggeration });
      const fail = (i: number) => {
        if (my === reqRef.current) speakSystem(id, sents.slice(i).join(" "), lang, onEnd);
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
      const speed = parseFloat(ls("aion.voice.speed") || "1") || 1;
      const exaggeration = parseFloat(ls("aion.voice.exaggeration") || "0.6") || 0.6;
      // HÍBRIDO: la voz clonada (Chatterbox) es ~3× tiempo real → demasiado lenta
      // para conversar EN VIVO. En modo voz / lectura automática usamos Piper mexicano
      // (instantáneo); la clonada queda para el botón Escuchar (a demanda).
      if (opts?.live && engine === "chatterbox") {
        engine = "piper";
        voiceName = "es_MX-claude-high";
      }
      // Voz clonada (lenta) → streaming por frases para empezar a hablar antes.
      if (engine === "chatterbox") {
        speakStreamed(id, clean, lang, voiceName, speed, exaggeration, my, onEnd);
        return;
      }
      // Voz propia de AION (Piper latino / Kokoro vía núcleo). Si el sidecar no está
      // o falla, cae a la voz del sistema sin romper la conversación.
      ttsSpeak(clean, lang, { voice: voiceName, engine, speed })
        .then((blob) => {
          if (my !== reqRef.current) return; // superada por otra orden / stop / barge-in
          return playTtsBlob(blob, () => {
            setSpeakingId((cur) => (cur === id ? null : cur));
            onEnd?.();
          }).catch(() => {
            // Audio bloqueado (sin gesto) o sin decodificar → voz del sistema.
            if (my === reqRef.current) speakSystem(id, clean, lang, onEnd);
          });
        })
        .catch(() => {
          // Sidecar caído / error de red → voz del sistema.
          if (my === reqRef.current) speakSystem(id, clean, lang, onEnd);
        });
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

  // ── Cola: reproduce frases en ORDEN sin cancelar la anterior, con Piper
  // (instantáneo). Permite hablar la respuesta del LLM mientras se genera. ──
  const drainQueue = useCallback(() => {
    if (qBusyRef.current) return;
    const my = reqRef.current;
    const next = qRef.current.shift();
    if (next === undefined) {
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
    const speed =
      typeof localStorage !== "undefined"
        ? parseFloat(localStorage.getItem("aion.voice.speed") || "1") || 1
        : 1;
    // Voz rápida (Piper mexicano) para fluidez en vivo, salvo que el usuario tenga
    // una voz de catálogo rápida elegida (no clonada).
    const savedEngine =
      typeof localStorage !== "undefined" ? localStorage.getItem("aion.voice.engine") : null;
    const savedVoice =
      typeof localStorage !== "undefined" ? localStorage.getItem("aion.voice.name") : null;
    const fast = savedEngine && savedEngine !== "chatterbox";
    const engine = fast ? savedEngine! : "piper";
    const voice = fast && savedVoice ? savedVoice : "es_MX-claude-high";
    ttsSpeak(next, qLangRef.current, { voice, engine, speed })
      .then((blob) => (my === reqRef.current ? playTtsBlob(blob) : undefined))
      .catch(() => { /* frase fallida → se omite, sigue la conversación */ })
      .finally(() => {
        qBusyRef.current = false;
        if (my === reqRef.current) drainQueue();
      });
  }, []);

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
      qRef.current.push(clean);
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
    onUtterance,
    onBargeIn,
  }: {
    listen: boolean;
    watchBargeIn: boolean;
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
        const tick = () => {
          an.getByteTimeDomainData(buf);
          let sum = 0;
          for (let i = 0; i < buf.length; i++) {
            const v = (buf[i] - 128) / 128;
            sum += v * v;
          }
          const rms = Math.sqrt(sum / buf.length);
          // Umbral con histéresis: necesita varios frames seguidos por encima
          // para no dispararse con ruido puntual (ni con el eco residual de AION).
          if (rms > 0.14) {
            above++;
            if (above >= 3) {
              cb.current.onBargeIn();
              return; // deja de vigilar; el ciclo pasará a escuchar
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
