import type { CapacitorConfig } from "@capacitor/cli";

// AION móvil: empaqueta el export estático de la UI web (apps/web/out).
// El cómputo (LLM) en móvil se resuelve conectando al "nodo personal" (tu Mac)
// vía LAN — define NEXT_PUBLIC_BRIDGE_URL / NEXT_PUBLIC_CONTROL_URL al construir
// la web apuntando a la IP de tu Mac. (On-device con Gemma E2B/E4B: futuro.)
const config: CapacitorConfig = {
  appId: "it.prontoclick.aion",
  appName: "AION",
  webDir: "../web/out",
  ios: { contentInset: "always" },
  server: {
    androidScheme: "https",
  },
};

export default config;
