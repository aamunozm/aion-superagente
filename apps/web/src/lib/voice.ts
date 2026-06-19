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
const SILENT_WAV =
  "data:audio/wav;base64,UklGRsQAAABXQVZFZm10IBAAAAABAAEAQB8AAIA+AAACABAAZGF0YaAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
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
  a.src = SILENT_WAV;
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
    const url = URL.createObjectURL(blob);
    const done = () => { try { URL.revokeObjectURL(url); } catch { /* */ } };
    a.onended = () => { done(); onEnded?.(); resolve(); };
    a.onerror = () => { done(); reject(new Error(`medio (código ${a.error?.code ?? "?"})`)); };
    a.src = url;
    const p = a.play();
    if (p && typeof p.catch === "function") {
      p.catch((e: unknown) => { done(); reject(new Error(`play: ${(e as Error)?.name || String(e)}`)); });
    }
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

  const cleanupAudio = useCallback(() => {
    stopPlayback();
  }, []);

  const stop = useCallback(() => {
    reqRef.current++; // cualquier voz en preparación queda invalidada
    if (typeof window !== "undefined" && "speechSynthesis" in window) {
      window.speechSynthesis.cancel();
    }
    cleanupAudio();
    setSpeakingId(null);
  }, [cleanupAudio]);

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

  const speak = useCallback(
    (id: string, text: string, lang: Lang, onEnd?: () => void) => {
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
      const voiceName =
        typeof localStorage !== "undefined" ? localStorage.getItem("aion.voice.name") || "" : "";
      const speed =
        typeof localStorage !== "undefined"
          ? parseFloat(localStorage.getItem("aion.voice.speed") || "1") || 1
          : 1;
      // Voz propia de AION (Kokoro/Chatterbox vía núcleo). Si el sidecar no está o
      // falla, cae a la voz del sistema sin romper la conversación.
      ttsSpeak(clean, lang, { voice: voiceName, speed })
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
    [cleanupAudio, speakSystem],
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

  return { speak, stop, speakingId, supported };
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
