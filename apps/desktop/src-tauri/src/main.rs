//! Shell de escritorio de AION (Tauri v2).
//!
//! Carga la UI web (Next.js export) y la conecta con el núcleo. En esta primera
//! versión el núcleo corre como puente HTTP (`aion-core serve`); en una iteración
//! posterior se embebe como sidecar de Tauri y se exponen comandos nativos.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error al arrancar AION desktop");
}
