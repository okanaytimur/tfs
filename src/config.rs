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
///
/// Kimlik doğrulama sırası OpenSSH'inkini izler: **önce publickey, sonra
/// parola**. Ayrıntı için `ssh::Ssh::connect`.
#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    /// Listede görünen ad.
    pub name: String,
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    pub user: String,
    /// Parola. Anahtarla bağlanılıyorsa boş bırakılabilir.
    #[serde(default)]
    pub password: String,
    /// Özel anahtar dosyası (ör. `~/.ssh/id_ed25519`). Boşsa varsayılan
    /// anahtarlar (`id_ed25519`, `id_ecdsa`, `id_rsa`) denenir.
    #[serde(default)]
    pub key: Option<String>,
    /// Anahtar parolayla şifreliyse parolası.
    #[serde(default)]
    pub key_passphrase: Option<String>,
}

impl ServerConfig {
    /// `key` alanının `~` açılmış hâli.
    pub fn key_path(&self) -> Option<PathBuf> {
        self.key.as_deref().filter(|k| !k.is_empty()).map(expand_tilde)
    }
}

/// Baştaki `~`yi ev dizinine çevirir (JSON'a mutlak yol yazmak zorunda kalma).
pub fn expand_tilde(p: &str) -> PathBuf {
    let Some(rest) = p.strip_prefix("~/").or_else(|| p.strip_prefix("~\\")) else {
        return PathBuf::from(p);
    };
    match home_dir() {
        Some(h) => h.join(rest),
        None => PathBuf::from(p),
    }
}

/// Ev dizini (Unix `HOME`, Windows `USERPROFILE`).
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parola_ve_anahtar_opsiyonel_port_varsayilan() {
        let c: Config = serde_json::from_str(
            r#"{"servers":[{"name":"A","host":"h","user":"u","password":"p"}]}"#,
        )
        .unwrap();
        let s = &c.servers[0];
        assert_eq!(s.port, 22, "port verilmezse 22");
        assert!(s.key.is_none());
        assert!(s.key_passphrase.is_none());
    }

    /// Anahtarla bağlanan sunucuda `password` hiç yazılmayabilmeli — eskiden
    /// zorunluydu, config'i kıracak bir değişiklik olmasın.
    #[test]
    fn parolasiz_anahtar_tanimi_okunur() {
        let c: Config = serde_json::from_str(
            r#"{"servers":[{"name":"A","host":"h","user":"u","key":"~/.ssh/id_ed25519"}]}"#,
        )
        .unwrap();
        let s = &c.servers[0];
        assert_eq!(s.password, "");
        assert_eq!(s.key.as_deref(), Some("~/.ssh/id_ed25519"));
    }

    #[test]
    fn tilde_ev_dizinine_acilir() {
        let home = home_dir().expect("testte ev dizini olmalı");
        assert_eq!(expand_tilde("~/.ssh/id_rsa"), home.join(".ssh").join("id_rsa"));
        // Mutlak ve göreli yollara dokunulmaz.
        assert_eq!(expand_tilde("/etc/key"), PathBuf::from("/etc/key"));
        assert_eq!(expand_tilde("anahtar"), PathBuf::from("anahtar"));
        // Yalnızca "~/" öneki açılır; "~abc" bir kullanıcı adıdır, bize ait değil.
        assert_eq!(expand_tilde("~baskasi/x"), PathBuf::from("~baskasi/x"));
    }

    #[test]
    fn key_path_bos_stringi_yok_sayar() {
        let c: Config = serde_json::from_str(
            r#"{"servers":[{"name":"A","host":"h","user":"u","key":""}]}"#,
        )
        .unwrap();
        assert!(c.servers[0].key_path().is_none(), "boş `key` anahtar sayılmamalı");
    }

    /// Gömülü şablon her zaman geçerli JSON olmalı — ilk çalıştırmada bu dosya
    /// yazılıyor, bozuksa kullanıcı doğrudan hataya çarpar.
    #[test]
    fn gomulu_sablon_gecerli() {
        let c: Config = serde_json::from_str(TEMPLATE).expect("şablon JSON olarak geçerli olmalı");
        assert!(!c.servers.is_empty());
        assert!(
            c.servers.iter().any(|s| s.key.is_some()),
            "şablon anahtarla bağlanma örneğini de göstermeli"
        );
    }
}
