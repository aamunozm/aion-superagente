# AION — Frontera: "consciencia" funcional, autonomía y creatividad (8 jun 2026)

> 25/25 afirmaciones verificadas (votación adversarial 3-0 unánime, fuentes primarias arXiv,
> varias peer-reviewed: ICML 2024, Royal Society A, NeurIPS). Verificación del workflow OK esta vez.

## Veredicto honesto
**Ningún paper afirma consciencia real.** Todos usan "funcional", "tipo-", "metacognición
limitada". Lo construible hoy son **mecanismos acotados demostrados**, no una mente.

## DEMOSTRADO (con código / resultados) — construir sobre esto

| Mecanismo | Resultado | Fuente | Uso en AION |
|-----------|-----------|--------|-------------|
| **Self-modeling** (auto-modelo) | Predecir los propios estados internos **simplifica y regulariza** la red (RLCT baja), dose-dependiente | arXiv:2407.10188 (Royal Society A) | Tarea auxiliar barata → "auto-percepción" útil |
| **Attention Schema (ASA)** | Mejora teoría de la mente: interpreta mejor la atención ajena y se hace más interpretable | arXiv:2411.00983 (Princeton/Graziano) | Capa de auto-modelo + ToM para multi-agente |
| **Metacognición en LLMs** | REAL pero LIMITADA: monitorean/controlan un **subespacio de baja dimensión** de sus activaciones; mejora con in-context (N≤256) | arXiv:2505.13763, 2509.21545 | Auto-reflexión calibrada (sin sobrevender) |
| **MAGELLAN** (FLOWERS/INRIA) | Motivación intrínseca por *learning progress*; único método que dominó 25.000 objetivos evolutivos | arXiv:2502.07709 · **código abierto** | 🎯 **Núcleo de curiosidad/auto-objetivos** |
| **HERAKLES** (FLOWERS) | RL jerárquico autotélico: compila habilidades dominadas en red pequeña; LLM controlador de alto nivel | arXiv:2508.14751 | 🎯 **Arquitectura de skills auto-evolutivas** |
| **Motivación intrínseca LLM** | Reward LLM + novelty VAE mejora exploración (⚠️ solo 3 runs, débil) | arXiv:2508.18420 | Señal de exploración (experimental) |

## CONCEPTUAL / ESPECULATIVO — inspiración, no producto

| Idea | Estado | Fuente |
|------|--------|--------|
| **Global Workspace (GWA / "Theater of Mind")** | Bucle cognitivo de 4 fases (Perceive→Think→Arbitrate→Update). **SIN experimentos** (preprint 1 autor) | arXiv:2604.08206 |
| **Open-endedness = esencial para ASI** | Argumento de DeepMind (Hughes et al.): ingredientes ya existen | arXiv:2406.04268 (ICML 2024) |
| **Agentes autotélicos > optimización utilidad** | Posición: exploración auto-dirigida | arXiv:2510.14548 (NeurIPS WS) |
| **ASAL** (Sakana) — automated search for artificial life | Foundation models como motor de open-endedness | asal.sakana.ai |

## ⛔ El muro real (cuello de botella práctico)
**Lifelong learning sin olvido catastrófico NO está resuelto.** Hasta **GPT-5 saca 17.9/100**
en el benchmark StuLife (arXiv:2508.19005). → La memoria persistente auto-motivada es EL
problema abierto. Aquí es donde la **memoria darwiniana de AION es investigación de frontera**,
no ingeniería rutinaria.

## Recomendación de integración para AION
1. **Curiosidad/auto-objetivos** → adaptar MAGELLAN (learning progress) como motor de
   motivación intrínseca. Código abierto, encaja con núcleo local.
2. **Skills auto-evolutivas** → patrón HERAKLES (LLM alto nivel + políticas pequeñas compiladas)
   sobre nuestro sandbox WASM/Extism.
3. **Auto-modelo barato** → self-modeling como tarea auxiliar (2407.10188): "auto-percepción"
   con beneficio de eficiencia comprobado.
4. **Metacognición calibrada** → auto-reflexión consciente de sus límites (no fingir introspección plena).
5. **Bucle cognitivo** → inspirarse en GWA (4 fases) como orquestación, sabiendo que es no-validado.

> Hallazgo estratégico: **nadie ha integrado** consciencia-funcional + autonomía-profunda +
> memoria-evolutiva en un solo sistema. Esa integración es, literalmente, terreno inexplorado
> y la oportunidad real de AION.
