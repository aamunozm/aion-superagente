"use client";

// Visor de imágenes a pantalla completa (lightbox) compartido. Cualquier imagen del
// chat —foto adjunta por Ariel, screenshot o cara que reconoce el agente— se amplía al
// hacer clic. Provider + hook desacoplados para que el Markdown (molécula) y las páginas
// lo usen sin acoplarse a un organismo concreto.

import { createContext, useCallback, useContext, useEffect, useState } from "react";

type LightboxCtx = { open: (src: string, alt?: string) => void };

// Por defecto (sin provider) abre la imagen en una pestaña — degradación segura.
const Ctx = createContext<LightboxCtx>({
  open: (src) => {
    if (typeof window !== "undefined") window.open(src, "_blank", "noopener");
  },
});

export const useLightbox = () => useContext(Ctx);

export function LightboxProvider({ children }: { children: React.ReactNode }) {
  const [img, setImg] = useState<{ src: string; alt?: string } | null>(null);

  const open = useCallback((src: string, alt?: string) => setImg({ src, alt }), []);
  const close = useCallback(() => setImg(null), []);

  // Cerrar con Escape — gesto esperado en un visor a pantalla completa.
  useEffect(() => {
    if (!img) return;
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && close();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [img, close]);

  return (
    <Ctx.Provider value={{ open }}>
      {children}
      {img && (
        <div
          onClick={close}
          className="fixed inset-0 z-50 flex items-center justify-center p-6"
          style={{ background: "rgba(20,18,15,0.82)", backdropFilter: "blur(4px)" }}
          role="dialog"
          aria-modal="true"
        >
          {/* eslint-disable-next-line @next/next/no-img-element */}
          <img
            src={img.src}
            alt={img.alt ?? "imagen"}
            onClick={(e) => e.stopPropagation()}
            style={{
              maxWidth: "92vw",
              maxHeight: "88vh",
              borderRadius: 14,
              boxShadow: "0 12px 48px rgba(0,0,0,0.5)",
              objectFit: "contain",
            }}
          />
          <button
            onClick={close}
            aria-label="Cerrar"
            className="absolute top-5 right-6 text-white/80 hover:text-white"
            style={{ fontSize: 28, lineHeight: 1 }}
          >
            ✕
          </button>
        </div>
      )}
    </Ctx.Provider>
  );
}
