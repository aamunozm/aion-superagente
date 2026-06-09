//! Shell de escritorio de AION (Tauri v2).
//!
//! Arranca TODO el backend automáticamente al abrir la app y lo detiene al cerrar.
//! Un solo doble-clic, sin terminal y **sin instalar nada** (ni Docker ni Ollama):
//!
//! 1. **Motor LLM Ollama EMBEBIDO** — runtime portátil vendorizado en
//!    `Resources/ollama-runtime/` (binario universal arm64+x86_64 → Mac Silicon e
//!    Intel). Se lanza en un puerto PRIVADO para no chocar con un Ollama externo.
//! 2. **Control-plane** (auth/licencias) como sidecar.
//! 3. **Núcleo** (chat/agente/memoria) como sidecar, apuntando al Ollama embebido.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::Child as StdChild;
use std::sync::Mutex;
use tauri::{Manager, RunEvent};
use tauri_plugin_shell::process::CommandChild;
use tauri_plugin_shell::ShellExt;

/// Puerto privado del Ollama embebido (poco común, evita choques con :11434).
const OLLAMA_HOST: &str = "127.0.0.1:11919";

/// Procesos hijos para detenerlos limpiamente al salir.
#[derive(Default)]
struct Sidecars {
    /// Sidecars Tauri (núcleo + control-plane).
    tauri: Mutex<Vec<CommandChild>>,
    /// Ollama embebido (proceso del sistema lanzado desde el recurso).
    ollama: Mutex<Option<StdChild>>,
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(Sidecars::default())
        .setup(|app| {
            let ollama_url = format!("http://{OLLAMA_HOST}");

            // 1) Motor LLM: Ollama EMBEBIDO desde Resources/ollama-runtime/.
            //    No es un sidecar Tauri porque necesita a su runner `llama-server`
            //    y dylibs como vecinos en la MISMA carpeta (cargan vía @loader_path).
            match app
                .path()
                .resolve("ollama-runtime/ollama", tauri::path::BaseDirectory::Resource)
            {
                Ok(ollama_bin) => {
                    match std::process::Command::new(&ollama_bin)
                        .arg("serve")
                        .env("OLLAMA_HOST", OLLAMA_HOST)
                        .spawn()
                    {
                        Ok(child) => {
                            *app.state::<Sidecars>().ollama.lock().unwrap() = Some(child);
                        }
                        Err(e) => eprintln!("AION: no se pudo lanzar Ollama embebido: {e}"),
                    }
                }
                Err(e) => eprintln!("AION: no encuentro ollama-runtime en Resources: {e}"),
            }

            let mut tauri_children = Vec::new();

            // 2) Control-plane (auth/licencias) en :8787.
            match app.shell().sidecar("aion-control-plane") {
                Ok(cmd) => match cmd.spawn() {
                    Ok((_rx, child)) => tauri_children.push(child),
                    Err(e) => eprintln!("AION: no se pudo lanzar control-plane: {e}"),
                },
                Err(e) => eprintln!("AION: sidecar control-plane no encontrado: {e}"),
            }

            // 3) Núcleo (chat/agente/memoria) en :8765, apuntando al Ollama embebido.
            match app.shell().sidecar("aion-core") {
                Ok(cmd) => match cmd
                    .args(["serve", "127.0.0.1:8765"])
                    .env("AION_OLLAMA_URL", &ollama_url)
                    .spawn()
                {
                    Ok((_rx, child)) => tauri_children.push(child),
                    Err(e) => eprintln!("AION: no se pudo lanzar el núcleo: {e}"),
                },
                Err(e) => eprintln!("AION: sidecar núcleo no encontrado: {e}"),
            }

            *app.state::<Sidecars>().tauri.lock().unwrap() = tauri_children;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error al arrancar AION desktop")
        .run(|app, event| {
            // Al salir, detener TODOS los procesos para no dejar huérfanos.
            if let RunEvent::ExitRequested { .. } | RunEvent::Exit = event {
                if let Some(state) = app.try_state::<Sidecars>() {
                    for child in state.tauri.lock().unwrap().drain(..) {
                        let _ = child.kill();
                    }
                    if let Some(mut ollama) = state.ollama.lock().unwrap().take() {
                        let _ = ollama.kill();
                    }
                }
            }
        });
}
