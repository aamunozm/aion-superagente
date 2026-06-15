"use client";

import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

// Renderiza markdown de las respuestas de AION (chat y agente) con soporte de TABLAS (GFM),
// encabezados, listas, negritas y código — como Claude/Comet/ChatGPT. Seguro por defecto:
// react-markdown NO renderiza HTML crudo, así que el texto del modelo no puede inyectar markup.
// El estilo vive en la clase `.md` de globals.css (usa los design tokens de AION).
export default function Markdown({ children }: { children: string }) {
  return (
    <div className="md">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          // Las tablas anchas hacen scroll horizontal en vez de desbordar la burbuja.
          table: ({ node, ...props }) => (
            <div className="md-table-wrap">
              <table {...props} />
            </div>
          ),
          // Los enlaces abren fuera y con rel seguro.
          a: ({ node, ...props }) => <a {...props} target="_blank" rel="noreferrer noopener" />,
        }}
      >
        {children}
      </ReactMarkdown>
    </div>
  );
}
