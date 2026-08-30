//! F4 — dosyayı [`fresh`](https://github.com/sinelaw/fresh) editöründe açma.
//!
//! Yerel paneldeki dosya doğrudan açılır. Uzak paneldeki dosya önce geçici bir
//! dizine indirilir, editörde açılır ve **içeriği değiştiyse** SFTP ile geri
//! yüklenir — yani uzak sunucuda editör kurulu olmasına gerek yoktur.
//!
//! Editör tam ekran bir TUI olduğu için kendi TUI'mizi askıya alırız
//! (`suspend`/`resume`): ham mod kapanır, alternatif ekrandan çıkılır ve
//! **olay akışı kapatılır** — aksi halde crossterm'in arka plan okuyucusu
//! tuşları editörden çalar (bkz. `suspend`).

use std::ffi::OsStr;
use std::io::{self, Read, Stdout, Write};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use ratatui::{
    backend::CrosstermBackend,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame, Terminal,
};

use crate::EventSource;

/// crates.io paket adı (kurulum için).
pub const CRATE: &str = "fresh-editor";

/// Çalıştırılabilir dosyanın adı (paket adından farklı).
const BIN: &str = "fresh";

/// Editörde açılmasına izin verilen en büyük uzak dosya. Düzenleme için dosya
/// indirilip geri yüklendiğinden, koca bir log dosyasını yanlışlıkla çekmemek
/// için üst sınır. (`fresh` gigabaytlık dosyaları açabilir; sınır bizim
/// transferimiz içindir.)
pub const MAX_EDIT_BYTES: u64 = 64 * 1024 * 1024;

/// Çalıştırılabilir `fresh`i bulur.
///
/// PATH'e ek olarak `~/.cargo/bin` ve `~/.local/bin` de taranır: kurulumu az
/// önce biz tetiklediysek yeni dizin bu sürecin PATH'inde olmayabilir.
pub fn locate() -> Option<PathBuf> {
    let exe = if cfg!(windows) { "fresh.exe" } else { BIN };

    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    if let Some(cargo_home) = cargo_bin_dir() {
        dirs.push(cargo_home);
    }
    if let Some(home) = home_dir() {
        dirs.push(home.join(".local").join("bin"));
    }

    dirs.into_iter()
        .map(|d| d.join(exe))
        .find(|p| is_executable(p))
}

/// `cargo binstall` mevcut mu? (Kurulum planını bu belirler.)
fn has_cargo_binstall() -> bool {
    which("cargo-binstall").is_some()
}

fn has_cargo() -> bool {
    which("cargo").is_some()
}

fn which(name: &str) -> Option<PathBuf> {
    let exe = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    };
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    if let Some(d) = cargo_bin_dir() {
        dirs.push(d);
    }
    dirs.into_iter()
        .map(|d| d.join(&exe))
        .find(|p| is_executable(p))
}

fn cargo_bin_dir() -> Option<PathBuf> {
    if let Some(ch) = std::env::var_os("CARGO_HOME") {
        return Some(PathBuf::from(ch).join("bin"));
    }
    home_dir().map(|h| h.join(".cargo").join("bin"))
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(p: &Path) -> bool {
    p.is_file()
}

// --- TUI askıya alma / geri yükleme ---

/// TUI'yi askıya alır: ham mod kapanır, alternatif ekrandan çıkılır ve
/// **olay akışı kapatılır**.
///
/// Olay akışını kapatmak şart: crossterm'in `EventStream`'i, `poll_next`
/// Pending döndükten sonra arka planda bir thread'i tty üzerinde **bloklayan**
/// bir okumaya sokar. Alt süreç çalışırken bu thread ayakta kalırsa kullanıcının
/// tuşları editöre değil bize gider. `EventStream` düşürülünce (`Drop`) bu
/// thread uyandırılıp kapatılır.
///
/// Akış `EventSource::shutdown` ile **düşürülür**, yerine hemen yenisi
/// konmaz — o thread global okuyucu kilidini tuttuğu için yeni bir
/// `EventStream` kurmak kilitlenirdi (ayrıntı: `EventSource` belgesi).
fn suspend(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    events: &mut EventSource,
) -> Result<()> {
    crate::restore_terminal(terminal)?;
    events.shutdown();
    Ok(())
}

/// TUI'yi geri getirir ve ekranı tazeler (alt süreç ne bıraktıysa silinsin).
fn resume(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    crate::enter_tui(terminal)?;
    Ok(())
}

/// Alt süreci **düz terminalde** (TUI askıda) çalıştırır ve bitmesini bekler.
async fn run_suspended<I, S>(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    events: &mut EventSource,
    program: &Path,
    args: I,
    wait_for_key: bool,
) -> Result<std::process::ExitStatus>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    suspend(terminal, events)?;

    let status = tokio::process::Command::new(program)
        .args(args)
        .status()
        .await;

    if wait_for_key {
        press_enter_to_continue();
    }
    // Süreç patlasa bile TUI geri gelmeli.
    resume(terminal)?;

    status.with_context(|| format!("çalıştırılamadı: {}", program.display()))
}

fn press_enter_to_continue() {
    let mut out = io::stdout();
    let _ = writeln!(out);
    let _ = write!(out, "Devam etmek için Enter'a basın… ");
    let _ = out.flush();
    let mut buf = [0u8; 1];
    // Enter (ya da EOF) bekle; tek bayt yeter.
    let _ = io::stdin().read(&mut buf);
}

// --- Kurulum (cargo binstall) ---

/// Kurulum bittikten sonraki durum.
pub enum Installed {
    /// `fresh` artık bulunabiliyor.
    Ok(PathBuf),
    /// Kurulum denendi ama `fresh` hâlâ yok.
    Failed(String),
}

/// Kullanıcıya gösterilecek kurulum planı (onay kutusunda yazar).
pub fn install_plan() -> Vec<String> {
    if has_cargo_binstall() {
        vec![format!("cargo binstall --no-confirm {CRATE}")]
    } else if has_cargo() {
        vec![
            "cargo install cargo-binstall".into(),
            format!("cargo binstall --no-confirm {CRATE}"),
            format!("(gerekirse) cargo install --locked {CRATE}"),
        ]
    } else {
        vec![]
    }
}

/// `fresh`i kurar. TUI askıya alınır, komutların çıktısı doğrudan terminalde
/// görünür (kurulum uzun sürebilir, kullanıcı ilerlemeyi görmeli).
///
/// Sıra: `cargo binstall` (hazır binary — hızlı) → yoksa önce `cargo-binstall`
/// kurulur → o da olmazsa son çare kaynaktan `cargo install --locked`.
pub async fn install(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    events: &mut EventSource,
) -> Result<Installed> {
    let Some(cargo) = which("cargo") else {
        return Ok(Installed::Failed(
            "cargo bulunamadı — Rust kurulu değil (https://rustup.rs)".into(),
        ));
    };

    suspend(terminal, events)?;

    let outcome = install_steps(&cargo).await;

    match &outcome {
        Ok(()) => println!("\n✓ {CRATE} kuruldu."),
        Err(e) => println!("\n✗ Kurulum başarısız: {e}"),
    }
    press_enter_to_continue();
    resume(terminal)?;

    Ok(match locate() {
        Some(p) => Installed::Ok(p),
        None => Installed::Failed(match outcome {
            Ok(()) => "kurulum bitti ama `fresh` bulunamadı (PATH?)".into(),
            Err(e) => e.to_string(),
        }),
    })
}

/// Kurulum adımları (TUI zaten askıda, çıktı doğrudan terminale gider).
async fn install_steps(cargo: &Path) -> Result<()> {
    println!("\n── {CRATE} kuruluyor ──\n");

    if !has_cargo_binstall() {
        println!("$ cargo install cargo-binstall");
        // Başarısız olursa ölümcül değil: aşağıda kaynaktan kuruluma düşeriz.
        let _ = run(cargo, ["install", "cargo-binstall"]).await;
    }

    if has_cargo_binstall() {
        println!("\n$ cargo binstall --no-confirm {CRATE}");
        if run(cargo, ["binstall", "--no-confirm", CRATE]).await.is_ok() {
            return Ok(());
        }
        println!("\nbinstall başarısız — kaynaktan derlemeye düşülüyor (uzun sürebilir).");
    }

    println!("\n$ cargo install --locked {CRATE}");
    run(cargo, ["install", "--locked", CRATE]).await
}

/// Bir komutu çalıştırır; sıfır olmayan çıkış kodunu hata sayar.
async fn run<I, S>(program: &Path, args: I) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let status = tokio::process::Command::new(program)
        .args(args)
        .status()
        .await
        .with_context(|| format!("çalıştırılamadı: {}", program.display()))?;
    if !status.success() {
        bail!("komut {status} ile bitti");
    }
    Ok(())
}

// --- Editörü açma ---

/// Dosyayı `fresh` ile açar ve editör kapanana kadar bekler.
pub async fn open(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    events: &mut EventSource,
    bin: &Path,
    file: &Path,
) -> Result<()> {
    let status = run_suspended(terminal, events, bin, [file.as_os_str()], false).await?;
    if !status.success() {
        bail!("editör {status} ile kapandı");
    }
    Ok(())
}

// --- Geçici dosya + değişiklik tespiti ---

/// Uzak dosyanın indirileceği geçici dizini açar ve içindeki dosya yolunu
/// döndürür. Dosya adı korunur — editör sözdizimi vurgusunu uzantıdan seçiyor.
///
/// Dizin **münhasıran** oluşturulur (`create_dir`, `create_dir_all` değil):
/// hazır bir dizini benimsersek başkasının (ya da eşzamanlı bir düzenlemenin)
/// dosyasının üzerine yazar, `cleanup_temp` ile de onu silerdik.
pub fn temp_file_for(name: &str) -> Result<PathBuf> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);

    let base = std::env::temp_dir();
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);

    let mut last_err = None;
    for _ in 0..64 {
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = base.join(format!("tfs-edit-{}-{stamp}-{n}", std::process::id()));
        match std::fs::create_dir(&dir) {
            Ok(()) => return Ok(dir.join(safe_file_name(name))),
            // Sıradaki numarayı dene.
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => last_err = Some(e),
            Err(e) => {
                return Err(anyhow::Error::new(e))
                    .with_context(|| format!("geçici dizin açılamadı: {}", dir.display()))
            }
        }
    }
    Err(anyhow::Error::new(last_err.expect("döngü en az bir kez döner")))
        .context("geçici dizin açılamadı (çakışma)")
}

/// Uzak taraftan gelen adı dosya adına indirger — yol ayırıcı ya da `..`
/// içeren bir "isim" geçici dizinin dışına yazmamalı.
fn safe_file_name(name: &str) -> &str {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("dosya");
    if base.is_empty() || base == "." || base == ".." {
        "dosya"
    } else {
        base
    }
}

/// Düzenleme bitince geçici dizini (dosyasıyla birlikte) siler.
pub fn cleanup_temp(file: &Path) {
    if let Some(dir) = file.parent() {
        // Yalnızca bizim açtığımız dizini sil.
        if dir
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("tfs-edit-"))
        {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

/// Dosya içeriğinin FNV-1a 64-bit özeti. Editörden sonra dosyanın gerçekten
/// değişip değişmediğini anlamak için (zaman damgası güvenilmez: editör dosyayı
/// değiştirmeden kaydedebilir, ya da hiç kaydetmeyebilir).
pub fn content_hash(path: &Path) -> Result<u64> {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    let mut f = std::fs::File::open(path)
        .with_context(|| format!("dosya okunamadı: {}", path.display()))?;
    let mut buf = vec![0u8; 64 * 1024];
    let mut hash = OFFSET;
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        for b in &buf[..n] {
            hash ^= *b as u64;
            hash = hash.wrapping_mul(PRIME);
        }
    }
    Ok(hash)
}

// --- Onay kutusu (fresh kurulu değilken) ---

/// "fresh kurulu değil, kurulsun mu?" kutusu. Mevcut ekranın üzerine çizilir.
pub fn draw_install_prompt(f: &mut Frame, plan: &[String]) {
    let mut lines = vec![
        Line::from(Span::styled(
            "fresh editörü bulunamadı.",
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];

    if plan.is_empty() {
        lines.push(Line::from(
            "cargo da bulunamadı — önce Rust kurun: https://rustup.rs",
        ));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Esc: kapat",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        lines.push(Line::from("Şimdi kurulsun mu? Çalıştırılacak:"));
        for cmd in plan {
            lines.push(Line::from(Span::styled(
                format!("  $ {cmd}"),
                Style::default().fg(Color::LightCyan),
            )));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Kurulum birkaç dakika sürebilir; ilerleme ekranda görünür.",
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(
                " E / Enter ",
                Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" kur    "),
            Span::styled(
                " H / Esc ",
                Style::default().fg(Color::Black).bg(Color::Gray).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" vazgeç"),
        ]));
    }

    let height = lines.len() as u16 + 2;
    let area = centered(f.area(), 66, height);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        .title(" Editör kurulumu ");
    f.render_widget(Clear, area);
    f.render_widget(Paragraph::new(lines).block(block).wrap(Wrap { trim: false }), area);
}

fn centered(area: Rect, percent_x: u16, height: u16) -> Rect {
    let width = ((area.width as u32 * percent_x as u32 / 100) as u16).clamp(20, area.width.max(1));
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect {
        x,
        y,
        width,
        height: height.min(area.height),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_dosya_adi_yol_icermez() {
        let p = temp_file_for("../../etc/passwd").unwrap();
        assert_eq!(p.file_name().unwrap(), "passwd");
        assert!(p.parent().unwrap().starts_with(std::env::temp_dir()));
        cleanup_temp(&p);
    }

    #[test]
    fn temp_dosya_bos_isimde_de_gecerli() {
        let p = temp_file_for("").unwrap();
        assert_eq!(p.file_name().unwrap(), "dosya");
        cleanup_temp(&p);
    }

    /// Geçici dizin gerçekten siliniyor mu (aksi halde /tmp dolar).
    #[test]
    fn cleanup_gecici_dizini_siler() {
        let p = temp_file_for("a.txt").unwrap();
        std::fs::write(&p, b"x").unwrap();
        let dir = p.parent().unwrap().to_path_buf();
        assert!(dir.exists());
        cleanup_temp(&p);
        assert!(!dir.exists());
    }

    /// Bizim açmadığımız bir dizin asla silinmez.
    #[test]
    fn cleanup_yabanci_dizine_dokunmaz() {
        let dir = std::env::temp_dir().join("tfs-test-yabanci");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.txt");
        std::fs::write(&file, b"x").unwrap();
        cleanup_temp(&file);
        assert!(dir.exists(), "tfs-edit- ile başlamayan dizin silinmemeli");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Aynı milisaniyede iki çağrı çakışmamalı: geçici dizin münhasıran
    /// açılıyor, yoksa biri diğerinin dosyasını (ve dizinini) götürürdü.
    #[test]
    fn ayni_anda_iki_temp_dizini_cakismaz() {
        let a = temp_file_for("x.txt").unwrap();
        let b = temp_file_for("x.txt").unwrap();
        assert_ne!(a.parent().unwrap(), b.parent().unwrap());
        cleanup_temp(&a);
        cleanup_temp(&b);
    }

    #[test]
    fn hash_icerik_degisince_degisir() {
        let p = temp_file_for("h.txt").unwrap();
        std::fs::write(&p, b"merhaba").unwrap();
        let a = content_hash(&p).unwrap();
        assert_eq!(a, content_hash(&p).unwrap(), "aynı içerik → aynı özet");
        std::fs::write(&p, b"merhabb").unwrap();
        assert_ne!(a, content_hash(&p).unwrap());
        cleanup_temp(&p);
    }
}
