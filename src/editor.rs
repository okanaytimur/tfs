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
    if let Some(d) = install_dir() {
        dirs.push(d);
    }

    dirs.into_iter()
        .map(|d| d.join(exe))
        .find(|p| is_executable(p))
}

/// Hazır binary'nin kurulduğu dizin: Windows'ta `%LOCALAPPDATA%\tfs\bin`
/// (yönetici hakkı istemez, PATH'e eklemek gerekmez — `locate` oraya da
/// bakar), diğerlerinde `~/.local/bin` (çoğu dağıtımda zaten PATH'te).
fn install_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("tfs").join("bin"))
    } else {
        home_dir().map(|h| h.join(".local").join("bin"))
    }
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

/// Kullanıcıya gösterilecek kurulum planı (onay kutusunda yazar). Boşsa
/// kurmanın bir yolu yok.
pub fn install_plan() -> Vec<String> {
    let mut plan = Vec::new();
    if let (Some(asset), Some(_), Some(dir)) = (prebuilt_asset(), download_tools(), install_dir()) {
        plan.push(format!("GitHub'dan hazır binary indir: {asset}"));
        plan.push(format!("  → {}", dir.display()));
    }
    let yedek = if plan.is_empty() { "" } else { "(olmazsa) " };
    if has_cargo_binstall() {
        plan.push(format!("{yedek}$ cargo binstall --no-confirm {CRATE}"));
    } else if has_cargo() {
        plan.push(format!("{yedek}$ cargo install cargo-binstall"));
        plan.push(format!("{yedek}$ cargo binstall --no-confirm {CRATE}"));
        plan.push(format!("{yedek}$ cargo install --locked {CRATE}"));
    }
    plan
}

/// `fresh`i kurar. TUI askıya alınır, komutların çıktısı doğrudan terminalde
/// görünür (kullanıcı ilerlemeyi görmeli).
///
/// Sıra: **GitHub'daki hazır binary** (Rust gerekmez, saniyeler sürer) → o
/// olmazsa ve cargo varsa `cargo binstall` → son çare kaynaktan
/// `cargo install --locked`.
pub async fn install(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    events: &mut EventSource,
) -> Result<Installed> {
    let prebuilt = match (prebuilt_asset(), download_tools(), install_dir()) {
        (Some(asset), Some(tools), Some(dir)) => Some((asset, tools, dir)),
        _ => None,
    };
    let cargo = which("cargo");
    if prebuilt.is_none() && cargo.is_none() {
        return Ok(Installed::Failed(
            "bu platform için hazır binary yok (ya da curl/tar bulunamadı) ve cargo da \
             yok — Rust kurun: https://rustup.rs"
                .into(),
        ));
    }

    suspend(terminal, events)?;
    println!("\n── {CRATE} kuruluyor ──\n");

    let mut outcome = Err(anyhow::anyhow!("kurulum denenmedi"));
    if let Some((asset, tools, dir)) = &prebuilt {
        outcome = install_prebuilt(asset, tools, dir).await;
        if let Err(e) = &outcome {
            println!("\n✗ Hazır binary kurulamadı: {e:#}");
        }
    }
    if outcome.is_err() {
        if let Some(cargo) = &cargo {
            if prebuilt.is_some() {
                println!("\ncargo ile deneniyor…\n");
            }
            outcome = install_steps(cargo).await;
        }
    }

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

// --- Hazır binary (Rust gerekmez) ---

/// fresh'in GitHub sürümleri. `latest/download/<ad>` en son sürümün aynı adlı
/// dosyasına yönlenir; arşiv adları sürüm numarası taşımıyor, yani tfs'i her
/// fresh sürümünde güncellemek gerekmiyor.
const RELEASES: &str = "https://github.com/sinelaw/fresh/releases/latest/download";

/// İndirme/açma araçları: (`curl`, `tar`).
#[derive(Debug)]
pub struct Tools {
    curl: PathBuf,
    tar: PathBuf,
}

/// Bu makina için fresh'in yayınladığı arşivin adı (`None` = yayınlanmıyor).
///
/// Linux'ta **musl** (statik — glibc sürümü fark etmez) ve `.tar.gz` seçilir:
/// GNU tar `.xz`'i ancak `xz` kuruluysa açar, gzip ise her yerde var. macOS'ta
/// yalnızca `.tar.xz` var, ama oradaki bsdtar onu kendisi açar.
fn prebuilt_asset() -> Option<&'static str> {
    Some(match (std::env::consts::OS, machine_arch()?) {
        ("windows", "x86_64") => "fresh-editor-x86_64-pc-windows-msvc.zip",
        ("windows", "aarch64") => "fresh-editor-aarch64-pc-windows-msvc.zip",
        ("linux", "x86_64") => "fresh-editor-x86_64-unknown-linux-musl.tar.gz",
        ("linux", "aarch64") => "fresh-editor-aarch64-unknown-linux-musl.tar.gz",
        ("macos", "x86_64") => "fresh-editor-x86_64-apple-darwin.tar.xz",
        ("macos", "aarch64") => "fresh-editor-aarch64-apple-darwin.tar.xz",
        ("freebsd", "x86_64") => "fresh-editor-x86_64-unknown-freebsd.tar.xz",
        _ => return None,
    })
}

/// Makinanın mimarisi — tfs'in derlendiği mimari değil. 32-bit `tfs.exe`
/// 64-bit Windows'ta da çalışır ve orada 64-bit fresh kurulabilir; WOW64
/// altında gerçek mimari `PROCESSOR_ARCHITEW6432`'dedir.
fn machine_arch() -> Option<&'static str> {
    if cfg!(windows) {
        let a = std::env::var("PROCESSOR_ARCHITEW6432")
            .or_else(|_| std::env::var("PROCESSOR_ARCHITECTURE"))
            .unwrap_or_default()
            .to_ascii_uppercase();
        return match a.as_str() {
            "AMD64" => Some("x86_64"),
            "ARM64" => Some("aarch64"),
            _ => None, // gerçekten 32-bit Windows: fresh yayınlamıyor
        };
    }
    match std::env::consts::ARCH {
        "x86_64" => Some("x86_64"),
        "aarch64" => Some("aarch64"),
        _ => None,
    }
}

/// `curl` ve `tar` var mı? İkisi de Windows 10 (1803+) ile birlikte gelir,
/// Linux/macOS'ta da hemen her zaman kurulu.
///
/// Windows'ta `tar` **mutlaka** `System32\tar.exe` (bsdtar) olmalı: PATH'te
/// önde duran Git'in GNU tar'ı zip açamaz ("This does not look like a tar
/// archive").
fn download_tools() -> Option<Tools> {
    let tar = if cfg!(windows) {
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        Some(PathBuf::from(root).join("System32").join("tar.exe")).filter(|p| p.is_file())
    } else {
        which("tar")
    }?;
    Some(Tools { curl: which("curl")?, tar })
}

/// Hazır arşivi indirir, SHA-256'sını doğrular, açar ve `fresh`i `dir`e
/// kopyalar. Çıktı doğrudan terminale gider (TUI askıda).
async fn install_prebuilt(asset: &str, tools: &Tools, dir: &Path) -> Result<()> {
    let work = temp_file_for("indirme")?
        .parent()
        .map(Path::to_path_buf)
        .context("geçici dizin")?;
    let r = install_prebuilt_in(asset, tools, dir, &work).await;
    let _ = std::fs::remove_dir_all(&work);
    r
}

async fn install_prebuilt_in(asset: &str, tools: &Tools, dir: &Path, work: &Path) -> Result<()> {
    let archive = work.join(asset);
    let sum_file = work.join(format!("{asset}.sha256"));
    let url = format!("{RELEASES}/{asset}");

    println!("$ curl -L {url}");
    // -f: HTTP hatası (404) dosyaya HTML yazmak yerine hata olsun.
    run(&tools.curl, [OsStr::new("-fL"), OsStr::new("--progress-bar"), OsStr::new("-o"),
        archive.as_os_str(), OsStr::new(&url)])
    .await
    .context("indirilemedi")?;
    run(&tools.curl, [OsStr::new("-fsSL"), OsStr::new("-o"), sum_file.as_os_str(),
        OsStr::new(&format!("{url}.sha256"))])
    .await
    .context("SHA-256 dosyası indirilemedi")?;

    let beklenen = parse_sha256_file(&std::fs::read_to_string(&sum_file)?)
        .context("SHA-256 dosyası okunamadı")?;
    let gercek = file_sha256(&archive)?;
    if beklenen != gercek {
        bail!("SHA-256 tutmuyor (beklenen {beklenen}, inen {gercek}) — dosya bozuk inmiş");
    }
    println!("✓ SHA-256 doğrulandı");

    let x = work.join("x");
    std::fs::create_dir(&x)?;
    run(&tools.tar, [OsStr::new("-xf"), archive.as_os_str(), OsStr::new("-C"), x.as_os_str()])
        .await
        .context("arşiv açılamadı")?;

    let exe = if cfg!(windows) { "fresh.exe" } else { BIN };
    let src = find_file(&x, exe).with_context(|| format!("arşivde {exe} yok"))?;
    std::fs::create_dir_all(dir)
        .with_context(|| format!("dizin oluşturulamadı: {}", dir.display()))?;
    let dst = dir.join(exe);
    std::fs::copy(&src, &dst).with_context(|| format!("kopyalanamadı: {}", dst.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dst, std::fs::Permissions::from_mode(0o755))?;
    }
    println!("✓ {}", dst.display());
    Ok(())
}

/// `<hex>  <dosya adı>` biçimindeki `.sha256` dosyasından özeti alır.
fn parse_sha256_file(s: &str) -> Option<String> {
    let h = s.split_whitespace().next()?.to_ascii_lowercase();
    (h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit())).then_some(h)
}

fn file_sha256(p: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut f = std::fs::File::open(p)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// `root` altında (alt dizinler dahil) adı `name` olan ilk dosya. Linux
/// arşivi her şeyi bir alt dizine koyuyor, Windows zip'i köke.
fn find_file(root: &Path, name: &str) -> Option<PathBuf> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).ok()?.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if e.file_name() == name {
                return Some(p);
            }
        }
    }
    None
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
            "Bu platform için hazır binary yok (ya da curl/tar bulunamadı) ve cargo \
             da yok — önce Rust kurun: https://rustup.rs",
        ));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Esc: kapat",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        lines.push(Line::from("Şimdi kurulsun mu? Yapılacak:"));
        for adim in plan {
            lines.push(Line::from(Span::styled(
                format!("  {adim}"),
                Style::default().fg(Color::LightCyan),
            )));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "İlerleme ekranda görünür (cargo ile derleme dakikalar sürebilir).",
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

    #[test]
    fn sha256_bilinen_deger() {
        let p = temp_file_for("ozet.txt").unwrap();
        std::fs::write(&p, b"abc").unwrap();
        assert_eq!(
            file_sha256(&p).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        cleanup_temp(&p);
    }

    #[test]
    fn sha256_dosyasi_okunur() {
        let h = "B373B4D767038BCFB59D66CFC6F9B04E7843EF33F732457E947C4E27CB7F1CE4";
        assert_eq!(
            parse_sha256_file(&format!("{h}  fresh-editor-x86_64-pc-windows-msvc.zip\n")),
            Some(h.to_ascii_lowercase())
        );
        assert_eq!(parse_sha256_file(""), None);
        assert_eq!(parse_sha256_file("<html>Not Found</html>"), None);
    }

    /// Linux arşivi binary'yi alt dizine koyuyor; derinde de bulunmalı.
    #[test]
    fn arsivde_binary_bulunur() {
        let p = temp_file_for("x").unwrap();
        let kok = p.parent().unwrap();
        let alt = kok.join("fresh-editor-x86_64-unknown-linux-musl");
        std::fs::create_dir_all(alt.join("icons")).unwrap();
        std::fs::write(alt.join("icons").join("fresh.png"), b"").unwrap();
        std::fs::write(alt.join("fresh"), b"").unwrap();
        assert_eq!(find_file(kok, "fresh"), Some(alt.join("fresh")));
        assert_eq!(find_file(kok, "yok"), None);
        cleanup_temp(&p);
    }

    /// Bu testin koştuğu makinalar (Windows/Linux x86_64) için arşiv var.
    #[test]
    fn bu_makina_icin_hazir_arsiv_var() {
        if cfg!(any(windows, target_os = "linux")) && cfg!(target_arch = "x86_64") {
            assert!(prebuilt_asset().is_some());
        }
    }

    /// Ağ gerektirir: `cargo test hazir_binary -- --ignored --nocapture`.
    /// Gerçek arşivi geçici bir dizine kurar ve çalıştırır.
    #[tokio::test]
    #[ignore]
    async fn hazir_binary_uctan_uca() {
        let asset = prebuilt_asset().expect("bu platform için arşiv");
        let tools = download_tools().expect("curl + tar");
        let p = temp_file_for("kurulum").unwrap();
        let dir = p.parent().unwrap().join("bin");
        install_prebuilt(asset, &tools, &dir).await.unwrap();
        let exe = dir.join(if cfg!(windows) { "fresh.exe" } else { BIN });
        let out = std::process::Command::new(&exe).arg("--version").output().unwrap();
        assert!(out.status.success(), "{out:?}");
        eprintln!("{}", String::from_utf8_lossy(&out.stdout).trim());
        cleanup_temp(&p);
    }
}
