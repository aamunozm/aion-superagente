import type { Config } from "tailwindcss";

// Design system de AION (inspirado en CEO·Intelligence): ink slate-900 + plasma teal.
const config: Config = {
  content: ["./src/**/*.{ts,tsx}"],
  future: { hoverOnlyWhenSupported: true },
  darkMode: "class",
  theme: {
    extend: {
      colors: {
        ink: { DEFAULT: "#0F172A", hover: "#1E293B" },
        accent: { DEFAULT: "#0FB5BA", hover: "#0C9499", dark: "#2DD4D9" },
        cog: {
          thinking: "#0FB5BA",
          dreaming: "#8B7DB8",
          evolving: "#5B8FA8",
          curious: "#C49A3D",
          idle: "#7A7A7A",
        },
      },
      fontFamily: {
        display: ["Space Grotesk", "system-ui", "sans-serif"],
        sans: ["Inter", "system-ui", "sans-serif"],
        mono: ["JetBrains Mono", "monospace"],
      },
      boxShadow: {
        elevated: "0 4px 20px rgba(0,0,0,0.06)",
        float: "0 8px 30px rgba(0,0,0,0.10)",
      },
      transitionTimingFunction: {
        premium: "cubic-bezier(0.22, 1, 0.36, 1)",
        spring: "cubic-bezier(0.34, 1.56, 0.64, 1)",
      },
    },
  },
  plugins: [],
};

export default config;
