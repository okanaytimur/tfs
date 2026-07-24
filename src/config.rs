//! config.json okuma: birden fazla SSH sunucusu, parolalarıyla birlikte.

use anyhow::{bail, Context, Result};
use serde::Deserialize;

fn default_port() -> u16 {
    22
}

/// Tek bir SSH sunucusu tanımı.
#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    /// Listede görünen ad.
    pub name: String,
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    pub user: String,
    pub password: String,
}

/// config.json kök yapısı.
#[derive(Debug, Deserialize)]
pub struct Config {
    pub servers: Vec<ServerConfig>,
}

impl Config {
    /// Verilen yoldan config.json yükler ve doğrular.
    pub fn load(path: &str) -> Result<Self> {
        let text = std::fs::read_to_string(path).with_context(|| {
            format!(
                "config dosyası okunamadı: {path}\n\
                 (config.example.json dosyasını config.json olarak kopyalayıp düzenleyin)"
            )
        })?;
        let cfg: Config =
            serde_json::from_str(&text).with_context(|| format!("{path} JSON hatası"))?;
        if cfg.servers.is_empty() {
            bail!("{path} içinde 'servers' listesi boş");
        }
        Ok(cfg)
    }
}
