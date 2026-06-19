"use client";

// Aviso sonoro in-app cuando AION te escribe algo nuevo. Tono suave generado con
// Web Audio (sin archivos). Respeta el ajuste del usuario (localStorage "aion_sound",
// activado por defecto). Si el navegador bloquea el audio sin gesto previo, degrada en
// silencio — nunca lanza.

const SOUND_KEY = "aion_sound";

export function soundEnabled(): boolean {
  if (typeof localStorage === "undefined") return true;
  return localStorage.getItem(SOUND_KEY) !== "0";
}
export function setSoundEnabled(on: boolean) {
  try {
    localStorage.setItem(SOUND_KEY, on ? "1" : "0");
  } catch {
    /* almacenamiento no disponible */
  }
}

let ctx: AudioContext | null = null;

/** Campanita breve y cálida (dos notas) — el aviso de que AION te habló. */
export function chime() {
  if (typeof window === "undefined" || !soundEnabled()) return;
  try {
    const AC = window.AudioContext || (window as any).webkitAudioContext;
    if (!AC) return;
    ctx = ctx ?? new AC();
    const now = ctx.currentTime;
    // Dos notas (A5 → C#6), envolvente suave, volumen discreto.
    [880, 1108.73].forEach((freq, i) => {
      const osc = ctx!.createOscillator();
      const gain = ctx!.createGain();
      osc.type = "sine";
      osc.frequency.value = freq;
      const t = now + i * 0.12;
      gain.gain.setValueAtTime(0, t);
      gain.gain.linearRampToValueAtTime(0.13, t + 0.02);
      gain.gain.exponentialRampToValueAtTime(0.0001, t + 0.35);
      osc.connect(gain).connect(ctx!.destination);
      osc.start(t);
      osc.stop(t + 0.4);
    });
  } catch {
    /* audio bloqueado/no disponible: silencio */
  }
}
