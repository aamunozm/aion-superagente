import type { Metadata } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: "AION",
  description: "Super-agente de IA local-first, autónomo y auto-evolutivo.",
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="es" suppressHydrationWarning>
      <body>{children}</body>
    </html>
  );
}
