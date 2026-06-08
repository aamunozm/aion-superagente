/** @type {import('next').NextConfig} */
const nextConfig = {
  reactStrictMode: true,
  // Export estático compatible con el empaquetado Tauri (desktop).
  output: process.env.AION_TAURI ? "export" : undefined,
};

export default nextConfig;
