//! config.json okuma **ve yazma**: birden fazla SSH sunucusu, parolalarıyla.
//!
//! Dosya yoksa hata vermek yerine örnek şablon o yola oluşturulur ve çağırana
//! "henüz doldurulmadı" denir (bkz. `load_or_create`); uygulama bunu boş
//! bağlantı listesi sayıp yöneticiyi açar. Yazma tarafı `Config::save` —
//! bağlantı yöneticisi her değişiklikte çağırır.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
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
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerConfig {
    /// Listede görünen ad.
    pub name: String,
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    pub user: String,
    /// Parola. Anahtarla bağlanılıyorsa boş bırakılabilir.
    ///
    /// Yazarken boşsa dosyaya hiç yazılmaz — anahtarla bağlanan bir sunucunun
    /// tanımında `"password": ""` satırı gürültüden başka bir şey değil.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub password: String,
    /// Özel anahtar dosyası (ör. `~/.ssh/id_ed25519`). Boşsa varsayılan
    /// anahtarlar (`id_ed25519`, `id_ecdsa`, `id_rsa`) denenir.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// Anahtar parolayla şifreliyse parolası.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_passphrase: Option<String>,
}

impl Default for ServerConfig {
    /// Bağlantı yöneticisinde "yeni bağlantı" için boş kalıp.
    fn default() -> Self {
        Self {
            name: String::new(),
            host: String::new(),
            port: default_port(),
            user: String::new(),
            password: String::new(),
            key: None,
            key_passphrase: None,
        }
    }
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
#[derive(Debug, Default, Deserialize, Serialize)]
pub struct Config {
    pub servers: Vec<ServerConfig>,
}

/// `load_or_create` sonucu: ya kullanılabilir yapılandırma, ya da henüz
/// doldurulmamış bir şablon.
pub enum Loaded {
    /// Gerçek sunucu bilgileri okundu.
    Ready(Config),
    /// Dosya yoktu (şablon yazıldı) ya da vardı ama hâlâ dokunulmamış
    /// şablondu. Çağıran bunu "bağlantı listesi boş" diye ele alır ve
    /// bağlantı yöneticisini boş listeyle açar.
    NeedsEditing,
}

impl Config {
    /// Yapılandırmayı yükler; dosya yoksa şablonu oluşturur.
    ///
    /// Dosya yoksa **hata vermez** — `TEMPLATE`'i verilen yola yazıp
    /// `NeedsEditing` döner; çağıran bağlantı yöneticisini boş listeyle açar.
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
            return Ok(Loaded::NeedsEditing);
        }

        let text = std::fs::read_to_string(p)
            .with_context(|| format!("config dosyası okunamadı: {path}"))?;

        // Dosya duruyor ama hiç düzenlenmemiş (bire bir şablon). Şablondaki
        // uydurma sunucuları listeye sokmanın anlamı yok — boş kabul edilir,
        // kullanıcı yöneticide ilk kaydını kurunca üzerlerine yazılır.
        if text.replace("\r\n", "\n") == TEMPLATE.replace("\r\n", "\n") {
            return Ok(Loaded::NeedsEditing);
        }

        let cfg: Config =
            serde_json::from_str(&text).with_context(|| format!("{path} JSON hatası"))?;
        // Liste boş olabilir: bağlantı yöneticisi (F5/[+ Yeni]) boş listeyle de
        // açılır, kullanıcı ilk bağlantısını orada kurar.
        Ok(Loaded::Ready(cfg))
    }

    /// Yapılandırmayı diske yazar (bağlantı yöneticisi her değişiklikten sonra
    /// çağırır).
    ///
    /// **Atomik**: önce yanına geçici dosya yazılır, sonra üzerine taşınır.
    /// Yarıda kesilen bir yazma, içinde parolalar duran config'i yarım JSON'a
    /// çevirmemeli. Unix'te dosya `0600`'e çekilir — düz metin parola tutuyor,
    /// grup/diğer okuyamasın.
    pub fn save(&self, path: &str) -> Result<()> {
        let p = Path::new(path);
        if let Some(dir) = p.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("dizin oluşturulamadı: {}", dir.display()))?;
        }

        let mut json = serde_json::to_string_pretty(self).context("config JSON'a çevrilemedi")?;
        json.push('\n');

        let tmp = p.with_extension("json.tmp");
        std::fs::write(&tmp, json.as_bytes())
            .with_context(|| format!("geçici dosya yazılamadı: {}", tmp.display()))?;
        restrict_permissions(&tmp);
        // Windows'ta da üzerine yazar (std `rename` MOVEFILE_REPLACE_EXISTING
        // kullanır), ayrıca önce silmeye gerek yok.
        std::fs::rename(&tmp, p).with_context(|| format!("config yazılamadı: {path}"))?;
        Ok(())
    }
}

/// Unix'te dosyayı yalnızca sahibi okuyabilsin (`0600`). Diğer platformlarda
/// karşılığı yok — sessizce atlanır. Başarısız olursa da yazma iptal edilmez:
/// config'i kaydedememek, izni sıkılaştıramamaktan daha kötü.
#[cfg(unix)]
fn restrict_permissions(p: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict_permissions(_p: &Path) {}

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

    /// Kaydet - oku turu: bağlantı yöneticisinin yazdığı dosya geri
    /// okunabilmeli ve parola aynen dönmeli.
    #[test]
    fn kaydedilen_config_geri_okunur() {
        let dir = std::env::temp_dir().join(format!("tfs-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let yol = dir.join("kayit.json");
        let yol_s = yol.to_string_lossy().to_string();

        let cfg = Config {
            servers: vec![
                ServerConfig {
                    name: "Parolalı".into(),
                    host: "h1".into(),
                    port: 2222,
                    user: "okan".into(),
                    // Sondaki boşluk bilerek: parola trim'lenmemeli.
                    password: "gizli parola ".into(),
                    ..Default::default()
                },
                ServerConfig {
                    name: "Anahtarlı".into(),
                    host: "h2".into(),
                    user: "root".into(),
                    key: Some("~/.ssh/id_ed25519".into()),
                    ..Default::default()
                },
            ],
        };
        cfg.save(&yol_s).unwrap();

        // Parolasız kayıt için `password` alanı hiç yazılmamalı.
        let ham = std::fs::read_to_string(&yol).unwrap();
        assert_eq!(
            ham.matches("\"password\"").count(),
            1,
            "yalnızca parolalı kayıt `password` yazmalı:\n{ham}"
        );

        let Loaded::Ready(geri) = Config::load_or_create(&yol_s).unwrap() else {
            panic!("kaydedilen dosya Ready olarak okunmalı");
        };
        assert_eq!(geri.servers.len(), 2);
        assert_eq!(geri.servers[0].password, "gizli parola ");
        assert_eq!(geri.servers[0].port, 2222);
        assert_eq!(geri.servers[1].password, "");
        assert_eq!(geri.servers[1].port, 22, "yazılmayan port 22'ye düşer");
        assert_eq!(geri.servers[1].key.as_deref(), Some("~/.ssh/id_ed25519"));

        // Yazma atomik: geçici dosya arkada kalmamalı.
        assert!(!yol.with_extension("json.tmp").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Boş liste artık geçerli — yönetici boş listeyle açılıp ilk bağlantıyı
    /// kurdurabilmeli (eskiden burada hata veriliyordu).
    #[test]
    fn bos_sunucu_listesi_hata_degil() {
        let dir = std::env::temp_dir().join(format!("tfs-cfg-bos-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let yol = dir.join("bos.json");
        std::fs::write(&yol, r#"{"servers":[]}"#).unwrap();
        let Loaded::Ready(c) = Config::load_or_create(&yol.to_string_lossy()).unwrap() else {
            panic!("boş liste Ready olmalı");
        };
        assert!(c.servers.is_empty());
        std::fs::remove_dir_all(&dir).ok();
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
