/**
 * AION Design Tokens (fuente de verdad en TS).
 * Espejo de tokens.css para consumo en Tailwind config y componentes React.
 * Identidad AION: ink slate-900 + acento plasma teal.
 */

export const tokens = {
  color: {
    ink: "#0F172A",
    inkHover: "#1E293B",
    accent: "#0FB5BA",        // plasma teal — identidad AION
    accentHover: "#0C9499",
    accentDark: "#2DD4D9",
    success: "#16a34a",
    warn: "#f59e0b",
    danger: "#ef4444",
    info: "#0ea5e9",
    // estados cognitivos del agente
    cog: {
      thinking: "#0FB5BA",
      dreaming: "#8B7DB8",
      evolving: "#5B8FA8",
      curious: "#C49A3D",
      idle: "#7A7A7A",
    },
  },
  font: {
    display: "Space Grotesk",
    sans: "Inter",
    mono: "JetBrains Mono",
  },
  radius: { input: 10, btn: 10, card: 12, pill: 9999 },
  shadow: {
    elevated: "0 4px 20px rgba(0,0,0,0.06)",
    float: "0 8px 30px rgba(0,0,0,0.10)",
  },
  ease: {
    premium: "cubic-bezier(0.22, 1, 0.36, 1)",
    spring: "cubic-bezier(0.34, 1.56, 0.64, 1)",
  },
  duration: { fast: 120, base: 200 },
} as const;

export type Tokens = typeof tokens;
