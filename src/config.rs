//! config.json okuma: birden fazla SSH sunucusu, parolalarıyla.
//!
//! Dosya yoksa hata vermek yerine örnek şablon o yola **oluşturulur** ve
//! kullanıcıdan doldurması istenir (bkz. `load_or_create`).

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

/// İlk çalıştırmada yazılan şablon. `config.example.json` derleme sırasında
/// gömülür — böylece `cargo install tfs-ssh` ile kurulan binary'de de vardır
/// (yanında kaynak dosya olmasa bile) ve şablon örnekle asla ayrışmaz.
const TEMPLATE: &str = include_str!("../config.example.json");

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

/// `load_or_create` sonucu: ya kullanılabilir yapılandırma, ya da kullanıcının
/// önce doldurması gereken bir şablon.
pub enum Loaded {
    /// Gerçek sunucu bilgileri okundu.
    Ready(Config),
    /// Şablon doldurulmalı. `created` = bu çalıştırmada yeni oluşturuldu mu
    /// (false ise dosya vardı ama hâlâ dokunulmamış şablondu).
    NeedsEditing { path: PathBuf, created: bool },
}

impl Config {
    /// Yapılandırmayı yükler; dosya yoksa şablonu oluşturur.
    ///
    /// Dosya yoksa **hata vermez** — `TEMPLATE`'i verilen yola yazıp
    /// `NeedsEditing` döner, böylece uygulama kullanıcıya dosyanın yolunu
    /// gösterip nazikçe çıkabilir.
    pub fn load_or_create(path: &str) -> Result<Loaded> {
        let p = Path::new(path);

        if !p.exists() {
            // Yol bir alt dizini gösteriyorsa (ör. `cfg/sunucular.json`) dizini
            // de oluştur — aksi halde write başarısız olur.
            if let Some(dir) = p.parent().filter(|d| !d.as_os_str().is_empty()) {
                std::fs::create_dir_all(dir)
                    .with_context(|| format!("dizin oluşturulamadı: {}", dir.display()))?;
            }
            std::fs::write(p, TEMPLATE)
                .with_context(|| format!("örnek yapılandırma oluşturulamadı: {path}"))?;
            return Ok(Loaded::NeedsEditing {
                path: display_path(p),
                created: true,
            });
        }

        let text = std::fs::read_to_string(p)
            .with_context(|| format!("config dosyası okunamadı: {path}"))?;

        // Dosya duruyor ama hiç düzenlenmemiş (bire bir şablon) — bağlanmayı
        // denemek yerine yine kullanıcıyı dosyaya yönlendir.
        if text.replace("\r\n", "\n") == TEMPLATE.replace("\r\n", "\n") {
            return Ok(Loaded::NeedsEditing {
                path: display_path(p),
                created: false,
            });
        }

        let cfg: Config =
            serde_json::from_str(&text).with_context(|| format!("{path} JSON hatası"))?;
        if cfg.servers.is_empty() {
            bail!("{path} içinde 'servers' listesi boş");
        }
        Ok(Loaded::Ready(cfg))
    }
}

/// Kullanıcıya gösterilecek mutlak yol. Windows'ta `canonicalize` `\\?\D:\...`
/// biçiminde uzun-yol önekli sonuç verir; okunabilirlik için o önek atılır.
fn display_path(p: &Path) -> PathBuf {
    let abs = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let s = abs.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(stripped) => PathBuf::from(stripped),
        None => abs,
    }
}
