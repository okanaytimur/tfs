//! Değiştirilebilir kısayollar (`config.json` → `"keys"`).
//!
//! Neden: F1/F2 gibi F-tuşları uzak masaüstünde (AnyDesk, Radmin, RDP) ve
//! `Fn` tuşu isteyen dizüstülerde güvenilir değil — yerel makina, uzak
//! masaüstü istemcisi ya da klavye onları yolda yutabiliyor. Bu yüzden mod
//! tuşlarının varsayılanı artık `Alt+1` / `Alt+2`: her terminalde tek bir
//! kaçış dizisi (`ESC 1`) olarak kodlanabilir ve uzak masaüstü istemcileri Alt
//! ile rakamı olduğu gibi iletir. Aynı gerekçeyle diğer eylemlerin de ilk
//! tuşu `Alt+harf` (ekranda gösterilen o); alışkanlık olan F-tuşları yedek
//! olarak çalışmaya devam eder.
//!
//! Biçim: `"Ctrl+T"`, `"Alt+1"`, `"F5"`, `"Ctrl+Shift+E"`. Bir eyleme birden
//! fazla tuş verilebilir (`["F5", "Ctrl+T"]`). Verilmeyen eylem varsayılanında
//! kalır.
//!
//! Tuş haritası açılışta bir kez kurulur (`install`) ve her yerden `get()` ile
//! okunur — ekranların hepsine parametre olarak taşımaya değmeyecek, program
//! boyunca değişmeyen bir değer.

use std::sync::OnceLock;

use anyhow::{bail, Result};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::{Deserialize, Serialize};

/// Bir eylem için tek tuş ya da tuş listesi — JSON'da ikisi de yazılabilsin.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum KeyList {
    One(String),
    Many(Vec<String>),
}

impl KeyList {
    fn specs(&self) -> Vec<&str> {
        match self {
            KeyList::One(s) => vec![s.as_str()],
            KeyList::Many(v) => v.iter().map(String::as_str).collect(),
        }
    }
}

/// `config.json`daki `"keys"` bölümü, **kullanıcının yazdığı hâliyle**.
///
/// Ham metin olarak tutulur ki bağlantı yöneticisi config'i yeniden yazarken
/// kullanıcının yazımı (`"ctrl+t"` ya da `["F5","Ctrl+T"]`) aynen korunsun.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct KeysConfig {
    /// SSH terminaline geç.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal: Option<KeyList>,
    /// Dosya moduna geç.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<KeyList>,
    /// Seçili dosyayı karşı panele aktar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transfer: Option<KeyList>,
    /// Düzenle: dosya modunda seçili dosyayı editörde aç, bağlantı
    /// yöneticisinde seçili bağlantının formunu aç.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edit: Option<KeyList>,
    /// Çıkış (dosya modu ve bağlantı yöneticisi). `Esc` her zaman ayrıca çalışır.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quit: Option<KeyList>,
    /// Bağlantı yöneticisi: yeni bağlantı.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new: Option<KeyList>,
    /// Bağlantı yöneticisi: seçili bağlantıyı çoğalt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copy: Option<KeyList>,
    /// Bağlantı yöneticisi: seçili bağlantıyı sil (onay sorulur).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delete: Option<KeyList>,
    /// Bağlantı yöneticisi: seçili bağlantıyı yukarı taşı.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub move_up: Option<KeyList>,
    /// Bağlantı yöneticisi: seçili bağlantıyı aşağı taşı.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub move_down: Option<KeyList>,
    /// Düzenleme formu: kaydet. `Enter` her zaman ayrıca çalışır.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub save: Option<KeyList>,
    /// Düzenleme formu: parolaları göster/gizle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_password: Option<KeyList>,
}

impl KeysConfig {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Tek bir tuş kombinasyonu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding {
    code: KeyCode,
    mods: KeyModifiers,
}

impl Binding {
    /// `"Ctrl+Alt+T"` gibi bir yazımı çözer. Büyük/küçük harf önemsiz.
    pub fn parse(spec: &str) -> Result<Self> {
        let spec = spec.trim();
        if spec.is_empty() {
            bail!("boş kısayol");
        }
        // Sondaki "+" tuşun kendisi olabilir ("Ctrl++"), ayırıcı değil.
        let (head, key) = match spec.strip_suffix("++") {
            Some(h) => (h, "+"),
            None => match spec.rsplit_once('+') {
                Some((h, k)) => (h, k),
                None => ("", spec),
            },
        };

        let mut mods = KeyModifiers::NONE;
        for m in head.split('+').map(str::trim).filter(|m| !m.is_empty()) {
            mods |= match m.to_ascii_lowercase().as_str() {
                "ctrl" | "control" | "ctl" => KeyModifiers::CONTROL,
                "alt" | "meta" | "option" => KeyModifiers::ALT,
                "shift" => KeyModifiers::SHIFT,
                _ => bail!("'{spec}': bilinmeyen değiştirici '{m}' (Ctrl, Alt, Shift)"),
            };
        }

        let code = parse_key(key.trim()).ok_or_else(|| {
            anyhow::anyhow!("'{spec}': bilinmeyen tuş '{}'", key.trim())
        })?;

        if let KeyCode::Char(c) = code {
            if mods.contains(KeyModifiers::SHIFT) && !c.is_alphabetic() {
                bail!("'{spec}': Shift+simge yerine simgenin kendisini yazın (ör. \"Alt+!\")");
            }
            // Düz harf kısayol olamaz: dosya modunda harfler aramaya, terminalde
            // kabuğa gider — kısayol yazmayı çalardı.
            if !mods.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) {
                bail!("'{spec}': yazılabilir bir tuş tek başına kısayol olamaz, Ctrl ya da Alt ekleyin");
            }
        }

        Ok(normalize(code, mods))
    }

    /// Tuş olayı bu kombinasyon mu?
    pub fn matches(&self, k: &KeyEvent) -> bool {
        normalize(k.code, k.modifiers) == *self
    }

    /// Ekranda gösterilecek ad: `"Alt+1"`, `"Ctrl+T"`, `"F5"`.
    pub fn label(&self) -> String {
        let mut s = String::new();
        if self.mods.contains(KeyModifiers::CONTROL) {
            s.push_str("Ctrl+");
        }
        if self.mods.contains(KeyModifiers::ALT) {
            s.push_str("Alt+");
        }
        if self.mods.contains(KeyModifiers::SHIFT) {
            s.push_str("Shift+");
        }
        match self.code {
            KeyCode::Char(' ') => s.push_str("Space"),
            KeyCode::Char(c) => s.extend(c.to_uppercase()),
            KeyCode::F(n) => s.push_str(&format!("F{n}")),
            KeyCode::Enter => s.push_str("Enter"),
            KeyCode::Esc => s.push_str("Esc"),
            KeyCode::Tab => s.push_str("Tab"),
            KeyCode::Backspace => s.push_str("Backspace"),
            KeyCode::Insert => s.push_str("Ins"),
            KeyCode::Delete => s.push_str("Del"),
            KeyCode::Home => s.push_str("Home"),
            KeyCode::End => s.push_str("End"),
            KeyCode::PageUp => s.push_str("PgUp"),
            KeyCode::PageDown => s.push_str("PgDn"),
            KeyCode::Up => s.push('↑'),
            KeyCode::Down => s.push('↓'),
            KeyCode::Left => s.push('←'),
            KeyCode::Right => s.push('→'),
            _ => s.push('?'),
        }
        s
    }
}

fn parse_key(k: &str) -> Option<KeyCode> {
    let mut chars = k.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        // "Alt+F" Alt+f demektir; Shift istenirse açıkça yazılır.
        return Some(KeyCode::Char(c.to_lowercase().next().unwrap_or(c)));
    }
    let lower = k.to_ascii_lowercase();
    if let Some(n) = lower.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()) {
        return (1..=24).contains(&n).then_some(KeyCode::F(n));
    }
    Some(match lower.as_str() {
        "enter" | "return" => KeyCode::Enter,
        "esc" | "escape" => KeyCode::Esc,
        "tab" => KeyCode::Tab,
        "space" => KeyCode::Char(' '),
        "backspace" => KeyCode::Backspace,
        "ins" | "insert" => KeyCode::Insert,
        "del" | "delete" => KeyCode::Delete,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pgup" | "pageup" => KeyCode::PageUp,
        "pgdn" | "pagedown" => KeyCode::PageDown,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        _ => return None,
    })
}

/// Olay ile tanımı aynı biçime getirir. Terminaller aynı tuşu farklı
/// raporlayabiliyor: `Ctrl+Shift+T` kimi yerde `Char('T')+SHIFT`, kimi yerde
/// `Char('T')+CTRL` olarak gelir; `Shift+1` ise `Char('!')`dır. Kural: harfler
/// küçültülür ve büyük harf SHIFT sayılır; simgelerde SHIFT zaten simgenin
/// içindedir, atılır.
fn normalize(code: KeyCode, mods: KeyModifiers) -> Binding {
    let mut mods = mods & (KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT);
    let code = match code {
        KeyCode::Char(c) if c.is_uppercase() => {
            mods |= KeyModifiers::SHIFT;
            KeyCode::Char(c.to_lowercase().next().unwrap_or(c))
        }
        KeyCode::Char(c) if !c.is_alphabetic() => {
            mods.remove(KeyModifiers::SHIFT);
            KeyCode::Char(c)
        }
        KeyCode::BackTab => {
            mods |= KeyModifiers::SHIFT;
            KeyCode::Tab
        }
        c => c,
    };
    Binding { code, mods }
}

/// AltGr ile yazılmış bir karakter mi? Öyleyse karakterin kendisi.
///
/// Windows AltGr'yi **Ctrl+Alt** olarak bildirir. Türkçe Q klavyede `\ @ [ ]
/// { } | ~ €` hep AltGr'yle yazılır; bunlar Ctrl kombinasyonu sanılırsa `\`
/// kabuğa Ctrl+\ (0x1C, ekranda `^\`) olarak gider, `{` ESC'ye dönüşür.
/// Gerçek Ctrl+Alt+harf/rakam ise karakter olarak harfi/rakamı taşır — AltGr
/// hiçbir zaman düz ASCII harf ya da rakam üretmez, ayrım buradan yapılır.
pub fn altgr_char(k: &KeyEvent) -> Option<char> {
    let ca = KeyModifiers::CONTROL | KeyModifiers::ALT;
    match k.code {
        KeyCode::Char(c) if k.modifiers.contains(ca) && !c.is_ascii_alphanumeric() => Some(c),
        _ => None,
    }
}

/// Bir eyleme bağlı tuşlar.
#[derive(Debug, Clone)]
pub struct Keys(Vec<Binding>);

impl Keys {
    pub fn matches(&self, k: &KeyEvent) -> bool {
        self.0.iter().any(|b| b.matches(k))
    }

    /// İlk tuşun adı — ipuçlarında ve sekme etiketlerinde gösterilir.
    pub fn label(&self) -> String {
        self.0.first().map(Binding::label).unwrap_or_default()
    }
}

/// Çözülmüş tuş haritası.
#[derive(Debug, Clone)]
pub struct Keymap {
    pub terminal: Keys,
    pub files: Keys,
    pub transfer: Keys,
    pub edit: Keys,
    pub quit: Keys,
    pub new: Keys,
    pub copy: Keys,
    pub delete: Keys,
    pub move_up: Keys,
    pub move_down: Keys,
    pub save: Keys,
    pub show_password: Keys,
    /// Kullanıcının yazdığı ham bölüm — config yeniden yazılırken korunur.
    pub source: KeysConfig,
}

// Varsayılanlar: önce `Alt+…` (uzak masaüstünden sağlam geçer, ekranda o
// gösterilir), ardından alışkanlık olan F-tuşu. `Alt+F4` (pencereyi kapatır),
// `Alt+Enter` (Windows Terminal tam ekran), `Alt+Space` (sistem menüsü) ve
// `Alt+Tab` işletim sistemine ait — bilerek kullanılmadı.
const DEFAULT_TERMINAL: &[&str] = &["Alt+1"];
const DEFAULT_FILES: &[&str] = &["Alt+2"];
const DEFAULT_TRANSFER: &[&str] = &["Alt+T", "F5"];
const DEFAULT_EDIT: &[&str] = &["Alt+E", "F4"];
const DEFAULT_QUIT: &[&str] = &["Alt+Q", "Ctrl+Q", "F10"];
const DEFAULT_NEW: &[&str] = &["Alt+N", "F5", "Ins"];
const DEFAULT_COPY: &[&str] = &["Alt+C", "F6"];
const DEFAULT_DELETE: &[&str] = &["Alt+D", "F8", "Del"];
const DEFAULT_MOVE_UP: &[&str] = &["Alt+Up"];
const DEFAULT_MOVE_DOWN: &[&str] = &["Alt+Down"];
const DEFAULT_SAVE: &[&str] = &["Alt+S", "Ctrl+S", "F10"];
const DEFAULT_SHOW_PASSWORD: &[&str] = &["Alt+P", "F9"];

impl Default for Keymap {
    fn default() -> Self {
        Self::from_config(&KeysConfig::default()).expect("varsayılan kısayollar geçerli olmalı")
    }
}

impl Keymap {
    /// Config bölümünü çözer; hatalı bir tanım hangi eylemde olduğuyla bildirilir.
    pub fn from_config(c: &KeysConfig) -> Result<Self> {
        fn one(name: &str, given: &Option<KeyList>, default: &[&str]) -> Result<Keys> {
            let specs = match given {
                Some(l) => l.specs(),
                None => default.to_vec(),
            };
            if specs.is_empty() {
                bail!("keys.{name}: en az bir tuş verilmeli");
            }
            specs
                .into_iter()
                .map(|s| Binding::parse(s).map_err(|e| anyhow::anyhow!("keys.{name}: {e}")))
                .collect::<Result<Vec<_>>>()
                .map(Keys)
        }

        let map = Self {
            terminal: one("terminal", &c.terminal, DEFAULT_TERMINAL)?,
            files: one("files", &c.files, DEFAULT_FILES)?,
            transfer: one("transfer", &c.transfer, DEFAULT_TRANSFER)?,
            edit: one("edit", &c.edit, DEFAULT_EDIT)?,
            quit: one("quit", &c.quit, DEFAULT_QUIT)?,
            new: one("new", &c.new, DEFAULT_NEW)?,
            copy: one("copy", &c.copy, DEFAULT_COPY)?,
            delete: one("delete", &c.delete, DEFAULT_DELETE)?,
            move_up: one("move_up", &c.move_up, DEFAULT_MOVE_UP)?,
            move_down: one("move_down", &c.move_down, DEFAULT_MOVE_DOWN)?,
            save: one("save", &c.save, DEFAULT_SAVE)?,
            show_password: one("show_password", &c.show_password, DEFAULT_SHOW_PASSWORD)?,
            source: c.clone(),
        };

        // Aynı ekranda geçerli iki eylem aynı tuşu paylaşırsa biri hiç
        // çalışmaz. Farklı ekranlarda paylaşmak serbest (F5: dosya modunda
        // transfer, yöneticide yeni bağlantı). Mod tuşları her ekrana eklenir.
        let global = [("terminal", &map.terminal), ("files", &map.files)];
        let screens: [&[(&str, &Keys)]; 3] = [
            &[("transfer", &map.transfer), ("edit", &map.edit), ("quit", &map.quit)],
            &[
                ("new", &map.new),
                ("edit", &map.edit),
                ("copy", &map.copy),
                ("delete", &map.delete),
                ("move_up", &map.move_up),
                ("move_down", &map.move_down),
                ("quit", &map.quit),
            ],
            &[("save", &map.save), ("show_password", &map.show_password)],
        ];
        for screen in screens {
            let group: Vec<_> = global.iter().chain(screen).collect();
            for (i, (an, a)) in group.iter().enumerate() {
                for (bn, b) in &group[i + 1..] {
                    if let Some(x) = a.0.iter().find(|x| b.0.contains(x)) {
                        bail!("keys: '{}' hem {an} hem {bn} için tanımlı", x.label());
                    }
                }
            }
        }
        Ok(map)
    }
}

static KEYMAP: OnceLock<Keymap> = OnceLock::new();

/// Açılışta config okunduktan sonra bir kez çağrılır.
pub fn install(map: Keymap) {
    let _ = KEYMAP.set(map);
}

/// Geçerli tuş haritası (kurulmadıysa — ör. testlerde — varsayılanlar).
pub fn get() -> &'static Keymap {
    KEYMAP.get_or_init(Keymap::default)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, mods)
    }

    #[test]
    fn varsayilanlar_f1_f2_degil() {
        let m = Keymap::default();
        assert!(m.terminal.matches(&ev(KeyCode::Char('1'), KeyModifiers::ALT)));
        assert!(m.files.matches(&ev(KeyCode::Char('2'), KeyModifiers::ALT)));
        assert!(!m.terminal.matches(&ev(KeyCode::F(1), KeyModifiers::NONE)));
        assert!(!m.files.matches(&ev(KeyCode::F(2), KeyModifiers::NONE)));
        assert_eq!(m.terminal.label(), "Alt+1");
    }

    #[test]
    fn yazim_cozulur() {
        let b = Binding::parse("ctrl+t").unwrap();
        assert!(b.matches(&ev(KeyCode::Char('t'), KeyModifiers::CONTROL)));
        assert_eq!(b.label(), "Ctrl+T");
        assert!(!b.matches(&ev(KeyCode::Char('t'), KeyModifiers::NONE)));

        let f = Binding::parse(" F12 ").unwrap();
        assert!(f.matches(&ev(KeyCode::F(12), KeyModifiers::NONE)));

        let p = Binding::parse("Ctrl++").unwrap();
        assert!(p.matches(&ev(KeyCode::Char('+'), KeyModifiers::CONTROL)));
    }

    /// Terminaller Ctrl+Shift+harf'i farklı raporlar; ikisi de eşleşmeli.
    #[test]
    fn shift_harf_iki_bicimde_eslesir() {
        let b = Binding::parse("Ctrl+Shift+E").unwrap();
        let cs = KeyModifiers::CONTROL | KeyModifiers::SHIFT;
        assert!(b.matches(&ev(KeyCode::Char('E'), cs)));
        assert!(b.matches(&ev(KeyCode::Char('E'), KeyModifiers::CONTROL)));
        assert!(b.matches(&ev(KeyCode::Char('e'), cs)));
        assert!(!b.matches(&ev(KeyCode::Char('e'), KeyModifiers::CONTROL)));
    }

    #[test]
    fn hatali_tanimlar_reddedilir() {
        assert!(Binding::parse("t").is_err(), "düz harf aramayı çalar");
        assert!(Binding::parse("Shift+1").is_err());
        assert!(Binding::parse("Hyper+x").is_err());
        assert!(Binding::parse("Ctrl+Bilmem").is_err());
        assert!(Binding::parse("F25").is_err());
        assert!(Binding::parse("").is_err());
    }

    #[test]
    fn config_bolumu_okunur_ve_cakisma_yakalanir() {
        let c: KeysConfig =
            serde_json::from_str(r#"{"terminal":"Ctrl+Alt+T","files":["F2","Alt+2"]}"#).unwrap();
        let m = Keymap::from_config(&c).unwrap();
        assert!(m.terminal.matches(&ev(
            KeyCode::Char('t'),
            KeyModifiers::CONTROL | KeyModifiers::ALT
        )));
        assert!(m.files.matches(&ev(KeyCode::F(2), KeyModifiers::NONE)));
        // Verilmeyen eylemler varsayılanda kalır.
        assert!(m.transfer.matches(&ev(KeyCode::F(5), KeyModifiers::NONE)));

        let c: KeysConfig = serde_json::from_str(r#"{"terminal":"F5"}"#).unwrap();
        let e = Keymap::from_config(&c).unwrap_err().to_string();
        assert!(e.contains("terminal") && e.contains("transfer"), "{e}");

        let c: KeysConfig = serde_json::from_str(r#"{"quit":[]}"#).unwrap();
        assert!(Keymap::from_config(&c).is_err());
    }

    /// Varsayılanlar önce Alt'lı tuşu gösterir, F-tuşu yedek olarak çalışır.
    #[test]
    fn varsayilan_eylemler_alt_once() {
        let m = Keymap::default();
        let alt = |c| ev(KeyCode::Char(c), KeyModifiers::ALT);
        assert!(m.transfer.matches(&alt('t')));
        assert!(m.transfer.matches(&ev(KeyCode::F(5), KeyModifiers::NONE)));
        assert_eq!(m.transfer.label(), "Alt+T");
        assert!(m.edit.matches(&alt('e')));
        assert!(m.quit.matches(&alt('q')));
        assert!(m.new.matches(&alt('n')));
        assert!(m.new.matches(&ev(KeyCode::Insert, KeyModifiers::NONE)));
        assert!(m.copy.matches(&alt('c')));
        assert!(m.delete.matches(&ev(KeyCode::Delete, KeyModifiers::NONE)));
        assert!(m.move_up.matches(&ev(KeyCode::Up, KeyModifiers::ALT)));
        assert!(!m.move_up.matches(&ev(KeyCode::Up, KeyModifiers::NONE)));
        assert!(m.save.matches(&alt('s')));
        assert!(m.show_password.matches(&alt('p')));
    }

    /// Farklı ekranlardaki eylemler aynı tuşu paylaşabilir, aynı ekrandakiler
    /// paylaşamaz.
    #[test]
    fn cakisma_ekran_bazinda() {
        // Varsayılanda F5 hem transfer (dosya modu) hem new (yönetici) — geçerli.
        Keymap::default();
        // save (form) ile quit (liste) farklı ekran.
        let c: KeysConfig = serde_json::from_str(r#"{"save":"Alt+Q"}"#).unwrap();
        assert!(Keymap::from_config(&c).is_ok());
        // new ile copy aynı ekran.
        let c: KeysConfig = serde_json::from_str(r#"{"copy":"Alt+N"}"#).unwrap();
        let e = Keymap::from_config(&c).unwrap_err().to_string();
        assert!(e.contains("new") && e.contains("copy"), "{e}");
        // Mod tuşu her ekranla çakışır.
        let c: KeysConfig = serde_json::from_str(r#"{"files":"Alt+P"}"#).unwrap();
        assert!(Keymap::from_config(&c).is_err());
    }
}
