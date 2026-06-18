"use client";

// Voz de AION — 100% local en el navegador (Web Speech API). Cero backend, cero
// coste, nada sale del Mac. TTS para leer las respuestas; STT para hablarle.
// Degrada con elegancia: si el motor no existe (p. ej. WKWebView de Tauri sin
// reconocimiento), `supported` es false y la UI oculta el control.

import { useCallback, useEffect, useRef, useState } from "react";
import type { Lang } from "@/lib/i18n";

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

/**
 * Hook de SÍNTESIS (TTS). Un único controlador por página: pásalo a cada burbuja.
 * `speakingId` identifica qué mensaje se está leyendo (para alternar play/stop).
 */
export function useSpeech() {
  const [speakingId, setSpeakingId] = useState<string | null>(null);
  const supported = speechSupported();

  // Precarga la lista de voces (en Chrome llega async vía `voiceschanged`).
  useEffect(() => {
    if (!supported) return;
    window.speechSynthesis.getVoices();
    const warm = () => window.speechSynthesis.getVoices();
    window.speechSynthesis.addEventListener?.("voiceschanged", warm);
    return () => window.speechSynthesis.removeEventListener?.("voiceschanged", warm);
  }, [supported]);

  const stop = useCallback(() => {
    if (supported) window.speechSynthesis.cancel();
    setSpeakingId(null);
  }, [supported]);

  const speak = useCallback(
    (id: string, text: string, lang: Lang, onEnd?: () => void) => {
      if (!supported) return;
      const clean = stripMarkdownForSpeech(text);
      if (!clean) return;
      window.speechSynthesis.cancel();
      const u = new SpeechSynthesisUtterance(clean);
      u.lang = ttsLang(lang);
      const v = pickVoice(u.lang);
      if (v) u.voice = v;
      u.rate = 1.02;
      u.pitch = 1.0;
      u.onend = () => {
        setSpeakingId((cur) => (cur === id ? null : cur));
        onEnd?.();
      };
      u.onerror = () => setSpeakingId((cur) => (cur === id ? null : cur));
      setSpeakingId(id);
      window.speechSynthesis.speak(u);
    },
    [supported],
  );

  // Corta cualquier lectura al desmontar (no dejar a AION hablando solo).
  useEffect(() => () => { if (supported) window.speechSynthesis.cancel(); }, [supported]);

  return { speak, stop, speakingId, supported };
}

/**
 * Hook de DICTADO (STT). `onFinal` recibe la transcripción cuando el usuario
 * termina de hablar. `interim` muestra el texto provisional mientras habla.
 */
export function useDictation(lang: Lang, onFinal: (text: string) => void) {
  const supported = dictationSupported();
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
