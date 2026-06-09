/** @type {import('next').NextConfig} */
const nextConfig = {
  reactStrictMode: true,
  // Export estático (la UI siempre se empaqueta dentro de Tauri). Incondicional
  // para ser cross-platform: `next dev` lo ignora; el build genera /out.
  output: "export",
};

export default nextConfig;
