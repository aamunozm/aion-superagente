# Instalar AION en otro Mac (Apple Silicon)

Esta carpeta contiene todo lo necesario:

- **`AION.app`** — la app (chat + UI + Ollama/Gemma embebidos).
- **`provision-voice-mac.sh`** — instala el stack de **voz** local (no viaja en la app).
- **`INSTALAR-OTRO-MAC.md`** — esta guía.

> **Requisitos del Mac destino:** Apple Silicon (M1/M2/M3/M4), macOS reciente,
> conexión a internet y [Homebrew](https://brew.sh). Espacio libre: **~15 GB**
> (Gemma ~8 GB + voz ~3.5 GB + app ~0.7 GB).

---

## 1) Instalar la app

1. Abre **`AION.dmg`** y arrastra **AION** sobre la carpeta **Aplicaciones**.
2. **Ábrela la primera vez** (sin Terminal). Como la app está firmada *ad-hoc*
   (no con un Developer ID de pago de Apple), Gatekeeper la bloquea el primer
   arranque. Para autorizarla — **solo hace falta una vez**:

   - **macOS 14 o anterior:** clic derecho sobre **AION** en Aplicaciones →
     **Abrir** → en el aviso, **Abrir**.
   - **macOS 15 (Sequoia) o posterior:** haz doble clic (saldrá un aviso) →
     ve a **Ajustes del Sistema → Privacidad y seguridad** → baja hasta el
     mensaje sobre AION → pulsa **"Abrir igualmente"** → confirma.

   A partir de ahí, AION se abre normal con doble clic, como cualquier app.

   > Si (raro) aparece *"está dañada"* en vez de *"no identificada"*, es que el
   > archivo se transfirió con la firma alterada: vuelve a copiar el `.dmg` sin
   > recomprimir, o como último recurso ejecuta una vez en Terminal
   > `xattr -dr com.apple.quarantine /Applications/AION.app`.

3. Abre AION. **Concede los permisos** que pida el sistema:
   - **Micrófono** y **Reconocimiento de voz** (modo voz / dictado).
   - **Accesibilidad** y **Automatización** (si usas el control del Mac).
   - **Cámara** (solo si usas reconocimiento facial).

## 2) Primer arranque — chat de texto (automático)

En el primer arranque AION levanta su Ollama embebido y **descarga el modelo
Gemma (~8 GB)**. Puede tardar varios minutos según tu conexión. Mientras tanto el
chat puede ir lento o dar un aviso de "modelo no listo"; es normal la primera vez.

El chat de texto funciona **sin** el paso siguiente.

## 3) Provisionar la VOZ local (recomendado)

La voz optimizada (cerebro Qwen3-4B, Qwen3-TTS, Kokoro y las voces Piper
Diego/Mateo/Lucía/Daniela) **no viene en la app**. Para instalarla:

```bash
bash ~/Desktop/AION-instalador/provision-voice-mac.sh
```

> Ajusta la ruta si dejaste la carpeta en otro sitio. Descarga **~3.5 GB** y crea
> los entornos de Python con `uv` en `~/Library/Application Support/AION/`.
> Es **idempotente**: si se corta, vuelve a ejecutarlo y retoma.

Al terminar, **reinicia AION** (ciérrala del todo y vuelve a abrirla). El modo voz
usará el stack local:

- Cerebro de voz Qwen3-4B (`:11920`) · Qwen3-TTS (`:8768`) · Kokoro/Piper (`:8766`).
- Voz por defecto: **Mateo** (hombre). En **Ajustes → Voz** puedes elegir
  **Diego** (hombre, España), **Lucía** o **Daniela** (mujer).

### Verificar que la voz quedó instalada

```bash
ls "$HOME/Library/Application Support/AION/tts/piper-voices/"   # 4 .onnx + .json
"$HOME/Library/Application Support/AION/llm/venv/bin/python" -c "import mlx_lm; print('cerebro OK')"
```

Sin este paso, AION arranca igual pero el modo voz **cae a la voz del sistema**
de macOS (entendible, pero no es Diego/Mateo).

---

## 4) (Opcional) Updates estables en este Mac

La firma *ad-hoc* hace que macOS vuelva a pedir permisos en cada actualización.
Si vas a actualizar AION a menudo en este Mac, crea una identidad de firma local
estable (igual que en el Mac principal) y re-firma tras cada update — busca
"AION Local Signing" en la documentación del proyecto. No es necesario para usarla.

## Qué funciona y qué no (honesto)

- ✅ Chat de texto (Gemma local), UI completa, memoria, proyectos, herramientas.
- ✅ Voz local natural (tras el paso 3): Diego/Mateo hombre, Lucía/Daniela mujer.
- ⚠️ Requiere internet en la **primera** configuración (descarga de modelos).
- ❌ La voz local **no** funciona en Macs Intel (MLX es solo Apple Silicon).
