# AION móvil (Capacitor)

Empaqueta la **misma UI web** (`@aion/web`) como app nativa de **iOS/Android**.

## Estado
Scaffold listo (config + scripts). **Compilar requiere herramientas nativas tuyas:**
- iOS → **Xcode** (+ cuenta Apple Developer para firmar)
- Android → **Android Studio / SDK**

## Pasos
```bash
cd apps/mobile
pnpm install
pnpm sync            # construye la web (export) y la sincroniza a Capacitor
pnpm add:ios         # crea el proyecto iOS  (requiere Xcode)
pnpm open:ios        # abre en Xcode para compilar/ejecutar
# Android:
pnpm add:android && pnpm open:android
```

## Cómputo en móvil — "nodo personal"
Un iPhone no corre Gemma 4 12B cómodamente. Dos rutas:
1. **Nodo personal (recomendado ahora):** la app móvil se conecta por LAN al backend
   de tu Mac. Construye la web con las URLs de tu Mac:
   ```bash
   NEXT_PUBLIC_BRIDGE_URL=http://TU_IP_MAC:8765 \
   NEXT_PUBLIC_CONTROL_URL=http://TU_IP_MAC:8787 \
   AION_TAURI=1 pnpm --filter @aion/web build
   ```
2. **On-device (futuro):** modelo pequeño (Gemma E2B/E4B) vía MLX/llama.cpp + uniffi.

## Privacidad
La sincronización entre tu Mac y el móvil usa los **blobs cifrados E2E** de `aion-sync`.
