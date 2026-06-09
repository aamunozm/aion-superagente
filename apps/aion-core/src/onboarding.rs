//! Onboarding inteligente: escanea el hardware del equipo, deduce el "nivel"
//! (bajo / medio / superior) y recomienda el LLM local más adecuado, ofreciendo
//! alternativas. La UI usa esto para guiar la primera configuración.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemScan {
    pub os: String,
    pub arch: String,
    pub cpu_cores: usize,
    pub ram_gb: f64,
    pub disk_free_gb: f64,
    pub gpu: String,
    /// Nivel recomendado: "bajo" | "medio" | "superior".
    pub tier: String,
    pub tier_reason: String,
}

/// Escanea el equipo y deduce el nivel recomendado.
pub fn scan() -> SystemScan {
    use sysinfo::{Disks, System};
    let mut sys = System::new();
    sys.refresh_memory();
    sys.refresh_cpu_all();

    let ram_gb = sys.total_memory() as f64 / 1024.0 / 1024.0 / 1024.0;
    sys.refresh_cpu_all();
    let cpu_cores = sys.physical_core_count().unwrap_or_else(|| sys.cpus().len());

    let disk_free_gb = Disks::new_with_refreshed_list()
        .list()
        .iter()
        .map(|d| d.available_space())
        .max()
        .unwrap_or(0) as f64
        / 1024.0
        / 1024.0
        / 1024.0;

    let arch = std::env::consts::ARCH.to_string();
    let os = std::env::consts::OS.to_string();
    // GPU aproximada: Apple Silicon trae GPU Metal integrada potente.
    let gpu = if os == "macos" && arch == "aarch64" {
        "Apple Silicon (GPU Metal integrada)".to_string()
    } else if os == "macos" {
        "Mac Intel (sin GPU dedicada asumida)".to_string()
    } else {
        "GPU no detectada (se asume CPU)".to_string()
    };

    // Nivel por RAM (lo que más limita un LLM local).
    let (tier, tier_reason) = if ram_gb < 8.5 {
        (
            "bajo",
            format!("{ram_gb:.0} GB de RAM: conviene un modelo ligero para que vaya fluido."),
        )
    } else if ram_gb < 24.0 {
        (
            "medio",
            format!("{ram_gb:.0} GB de RAM: un modelo intermedio equilibra calidad y velocidad."),
        )
    } else {
        (
            "superior",
            format!("{ram_gb:.0} GB de RAM: puedes con un modelo grande de máxima calidad."),
        )
    };

    SystemScan {
        os,
        arch,
        cpu_cores,
        ram_gb: (ram_gb * 10.0).round() / 10.0,
        disk_free_gb: (disk_free_gb * 10.0).round() / 10.0,
        gpu,
        tier: tier.to_string(),
        tier_reason,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelOption {
    pub id: String,
    pub name: String,
    /// Nombre para `ollama pull` (o "gemma4-reason" para el modelo propio).
    pub ollama_name: String,
    pub size_gb: f64,
    pub tier: String,
    pub note: String,
    pub recommended: bool,
}

/// Catálogo de modelos LOCALES por nivel, marcando el recomendado para `tier`.
pub fn catalog(tier: &str) -> Vec<ModelOption> {
    let m = |id: &str, name: &str, ollama: &str, size: f64, t: &str, note: &str| ModelOption {
        id: id.into(),
        name: name.into(),
        ollama_name: ollama.into(),
        size_gb: size,
        tier: t.into(),
        note: note.into(),
        recommended: t == tier,
    };
    vec![
        // Bajo consumo.
        m("llama32-1b", "Llama 3.2 1B", "llama3.2:1b", 1.3, "bajo", "Muy ligero y rápido; ideal para equipos modestos."),
        m("gemma3-1b", "Gemma 3 1B", "gemma3:1b", 0.8, "bajo", "Minúsculo, arranca en cualquier equipo."),
        // Medio.
        m("gemma3-4b", "Gemma 3 4B", "gemma3:4b", 3.3, "medio", "Buen equilibrio calidad/velocidad."),
        m("llama31-8b", "Llama 3.1 8B", "llama3.1:8b", 4.7, "medio", "Más capaz; necesita algo más de RAM."),
        // Superior.
        m("gemma4-reason", "Gemma 4 12B · razonamiento (AION)", "gemma4-reason", 9.8, "superior", "El modelo propio de AION: sin censura, con razonamiento. Recomendado si tu equipo lo permite."),
        m("qwen25-14b", "Qwen 2.5 14B", "qwen2.5:14b", 9.0, "superior", "Alternativa potente de alta calidad."),
    ]
}
