//! Papelera **propia y reversible** de AION. En vez de borrar de verdad, los
//! archivos se mueven a `~/Library/Application Support/AION/trash/` con un
//! manifiesto que permite **restaurarlos** o purgarlos tras 30 días.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const RETENTION_DAYS: i64 = 30;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrashEntry {
    pub id: String,
    pub original_path: String,
    pub stored_path: String,
    pub deleted_at: DateTime<Utc>,
}

/// Papelera reversible respaldada por una carpeta + manifiesto JSONL.
pub struct AionTrash {
    dir: PathBuf,
    manifest: PathBuf,
}

impl AionTrash {
    /// Abre (o crea) la papelera en el directorio dado.
    pub fn open(dir: impl Into<PathBuf>) -> std::io::Result<Self> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir)?;
        let manifest = dir.join("manifest.jsonl");
        Ok(Self { dir, manifest })
    }

    /// Mueve un archivo a la papelera (NO lo borra). Devuelve la entrada creada.
    pub fn trash(&self, path: impl AsRef<Path>) -> std::io::Result<TrashEntry> {
        let path = path.as_ref();
        let id = uuid::Uuid::new_v4().to_string();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "archivo".into());
        let stored = self.dir.join(format!("{id}__{name}"));
        // Mover (rename) o copiar+borrar si cruza volúmenes.
        if std::fs::rename(path, &stored).is_err() {
            std::fs::copy(path, &stored)?;
            std::fs::remove_file(path)?;
        }
        let entry = TrashEntry {
            id,
            original_path: path.to_string_lossy().into_owned(),
            stored_path: stored.to_string_lossy().into_owned(),
            deleted_at: Utc::now(),
        };
        self.append(&entry)?;
        Ok(entry)
    }

    /// Restaura un archivo de la papelera a su ubicación original.
    pub fn restore(&self, id: &str) -> std::io::Result<()> {
        let entries = self.entries()?;
        let entry = entries.iter().find(|e| e.id == id).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "entrada no encontrada")
        })?;
        std::fs::rename(&entry.stored_path, &entry.original_path).or_else(|_| {
            std::fs::copy(&entry.stored_path, &entry.original_path)?;
            std::fs::remove_file(&entry.stored_path)
        })?;
        self.rewrite_without(id)?;
        Ok(())
    }

    /// Purga las entradas con más de 30 días (borrado definitivo). Devuelve nº.
    pub fn purge_expired(&self) -> std::io::Result<usize> {
        let cutoff = Utc::now() - Duration::days(RETENTION_DAYS);
        let entries = self.entries()?;
        let (expired, keep): (Vec<_>, Vec<_>) =
            entries.into_iter().partition(|e| e.deleted_at < cutoff);
        for e in &expired {
            let _ = std::fs::remove_file(&e.stored_path);
        }
        self.write_all(&keep)?;
        Ok(expired.len())
    }

    pub fn entries(&self) -> std::io::Result<Vec<TrashEntry>> {
        let text = match std::fs::read_to_string(&self.manifest) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(e),
        };
        Ok(text
            .lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect())
    }

    fn append(&self, entry: &TrashEntry) -> std::io::Result<()> {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.manifest)?;
        writeln!(f, "{}", serde_json::to_string(entry)?)
    }

    fn rewrite_without(&self, id: &str) -> std::io::Result<()> {
        let kept: Vec<_> = self.entries()?.into_iter().filter(|e| e.id != id).collect();
        self.write_all(&kept)
    }

    fn write_all(&self, entries: &[TrashEntry]) -> std::io::Result<()> {
        let body: String = entries
            .iter()
            .filter_map(|e| serde_json::to_string(e).ok())
            .map(|s| s + "\n")
            .collect();
        let tmp = self.manifest.with_extension("jsonl.tmp");
        std::fs::write(&tmp, body)?;
        std::fs::rename(&tmp, &self.manifest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trash_and_restore_roundtrip() {
        let base = std::env::temp_dir().join(format!("aion-trash-{}", uuid::Uuid::new_v4()));
        let work = base.join("work");
        std::fs::create_dir_all(&work).unwrap();
        let file = work.join("importante.txt");
        std::fs::write(&file, b"datos personales").unwrap();

        let trash = AionTrash::open(base.join("trash")).unwrap();
        let entry = trash.trash(&file).unwrap();
        assert!(!file.exists(), "el original debe haberse movido");
        assert_eq!(trash.entries().unwrap().len(), 1);

        trash.restore(&entry.id).unwrap();
        assert!(file.exists(), "debe restaurarse a su sitio");
        assert_eq!(std::fs::read(&file).unwrap(), b"datos personales");
        assert_eq!(trash.entries().unwrap().len(), 0);

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn purge_keeps_recent() {
        let base = std::env::temp_dir().join(format!("aion-trash-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&base).unwrap();
        let f = base.join("x.txt");
        std::fs::write(&f, b"x").unwrap();
        let trash = AionTrash::open(base.join("t")).unwrap();
        trash.trash(&f).unwrap();
        // Recién borrado: la purga no lo toca.
        assert_eq!(trash.purge_expired().unwrap(), 0);
        assert_eq!(trash.entries().unwrap().len(), 1);
        std::fs::remove_dir_all(&base).ok();
    }
}
