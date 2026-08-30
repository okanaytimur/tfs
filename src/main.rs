//! tfs (terminal-file-send) — SSH dosya tarayıcısı (F2) + modern SSH terminali (F1).
//!
//! Sunucular `config.json` dosyasından okunur (birden fazla sunucu, parolalarıyla).
//! Açılışta fareyle sunucu seçilir. Config yolu ilk argümanla değiştirilebilir:
//!   cargo run                 # ./config.json
//!   cargo run -- sunucular.json

mod app;
mod config;
mod editor;
mod picker;
mod ssh;
mod terminal;
mod ui;

use std::io::{self, Stdout};
use std::sync::Arc;

use anyhow::{Context, Result};
use crossterm::{
    event::{
        DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, EventStream, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures::StreamExt;
use ratatui::{backend::CrosstermBackend, Terminal};
use tokio::sync::mpsc;

use crate::app::{App, EditRequest, PanelId, TransferRequest, TransferState};
use crate::config::{Config, Loaded};
use crate::ssh::Ssh;
use crate::terminal::TermSession;

/// Olay akışı. `Peekable`, yapıştırma yığınını toplarken sıradaki olayı
/// tüketmeden yoklayabilmek için gerekli (bkz. `collect_key_burst`).
pub(crate) type Events = futures::stream::Peekable<EventStream>;

/// Olay akışının sahibi. Harici bir program (editör, kurulum) çalışırken akış
/// **kapatılır**, sonra tembel olarak yeniden kurulur.
///
/// Neden `Option` — yani neden akışı `mem::replace` ile değiştirmiyoruz:
/// crossterm'in `EventStream`'i kurulurken global okuyucu kilidini alır
/// (`stream.rs`: `lock_internal_event_reader().waker()`), o kilidi ise
/// **yoklanmış** bir akışın arka plan thread'i `poll_internal(None, …)` içinde
/// süresiz tutar (`event.rs`: zaman aşımı yoksa `lock_internal_event_reader()`
/// alınır ve bloklayan `poll` boyunca elde kalır). Dolayısıyla eskisini
/// düşürmeden yenisini kurmak **kilitlenir**: yeni akış, eski akışın thread'inin
/// beklediği kilidi bekler ve o thread'i uyandıracak olan `Drop` hiç çalışmaz.
/// Önce düşür, sonra kur.
pub(crate) struct EventSource(Option<Events>);

impl EventSource {
    fn new() -> Self {
        Self(Some(EventStream::new().peekable()))
    }

    /// Akışa erişim; kapatılmışsa yeniden kurar.
    pub(crate) fn get(&mut self) -> &mut Events {
        self.0.get_or_insert_with(|| EventStream::new().peekable())
    }

    /// Akışı düşürür: `EventStream::drop` okuyucu thread'ini uyandırır, thread
    /// tty'yi ve global kilidi bırakır. Alt süreç başlatmadan önce şart —
    /// yoksa kullanıcının tuşları ona değil bize gider.
    pub(crate) fn shutdown(&mut self) {
        self.0 = None;
    }
}

/// Aktif ekran (mod). F1 → Terminal, F2 → Dosya transferi.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    FileTransfer,
    Terminal,
}

#[tokio::main]
async fn main() -> Result<()> {
    let config_path = std::env::args().nth(1).unwrap_or_else(|| "config.json".into());
    let cfg = match Config::load_or_create(&config_path)? {
        Loaded::Ready(c) => c,
        // Yapılandırma yok ya da hiç doldurulmamış: şablonu gösterip nazikçe çık
        // (TUI hiç açılmadığı için düz yazdırmak güvenli).
        Loaded::NeedsEditing { path, created } => {
            print_config_hint(&path, created);
            return Ok(());
        }
    };

    let mut terminal = setup_terminal()?;
    // TUI içindeki tüm akışı sar; hata olsa da terminali geri yükle.
    let res = app_flow(&mut terminal, &cfg).await;
    restore_terminal(&mut terminal)?;
    res
}

/// Yapılandırma doldurulmadığında gösterilen yönlendirme. TUI açılmadan önce
/// (ya da hiç açılmadan) çağrılır, bu yüzden düz `println!` kullanılır.
fn print_config_hint(path: &std::path::Path, created: bool) {
    let p = path.display();
    println!();
    if created {
        println!("tfs — ilk çalıştırma");
        println!();
        println!("Yapılandırma dosyası bulunamadı, sizin için örnek bir tane oluşturuldu:");
    } else {
        println!("tfs — yapılandırma henüz düzenlenmemiş");
        println!();
        println!("Bu dosya hâlâ örnek şablonun aynısı:");
    }
    println!();
    println!("    {p}");
    println!();
    println!("Dosyayı açıp kendi sunucularınızı yazın (name / host / port / user /");
    println!("password), sonra tfs'i yeniden çalıştırın:");
    println!();
    if cfg!(windows) {
        println!("    notepad \"{p}\"");
    } else {
        println!("    ${{EDITOR:-nano}} \"{p}\"");
    }
    println!();
    println!("Farklı bir dosya kullanmak için yolunu argüman verin:  tfs sunucular.json");
    println!("Not: parolalar düz metin saklanır — dosyayı paylaşmayın, repoya koymayın.");
    println!();
}

/// Sunucu seç → bağlan → dosya tarayıcısını çalıştır.
async fn app_flow(terminal: &mut Terminal<CrosstermBackend<Stdout>>, cfg: &Config) -> Result<()> {
    let idx = match picker::run(terminal, &cfg.servers).await? {
        Some(i) => i,
        None => return Ok(()), // kullanıcı çıktı
    };
    let sc = &cfg.servers[idx];

    terminal.draw(|f| picker::draw_connecting(f, &sc.name))?;
    let ssh = Arc::new(
        Ssh::connect(&sc.host, sc.port, &sc.user, &sc.password)
            .await
            .with_context(|| format!("'{}' sunucusuna bağlanılamadı", sc.name))?,
    );

    let mut app = App::new();
    app.load_local(std::env::current_dir()?)?;
    let home = ssh.home().await?;
    app.load_remote(&ssh, home).await?;

    run(terminal, &mut app, ssh, sc.name.clone()).await
}

async fn run(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
    ssh: Arc<Ssh>,
    server_name: String,
) -> Result<()> {
    let mut events = EventSource::new();
    let mut screen = Screen::FileTransfer;

    // Kabuk oturumu ilk F1'de tembel açılır, sonra uygulama boyunca canlı tutulur.
    // Okuma yarısının alıcısı `term_rx`'te; `select!` bunu dinler.
    let mut term: Option<TermSession> = None;
    let mut term_rx: Option<mpsc::UnboundedReceiver<Vec<u8>>> = None;

    // Durum geçişleri `select!` içinde borrow çakışmasına yol açmasın diye
    // bayrakla işaretlenip döngü başında (select dışında) uygulanır.
    let mut pending_open = false;
    let mut shell_closed = false;
    // Yazılabilir bir tuş geldi; devamı (yapıştırma yığını olabilir) döngü
    // başında toplanacak. `events` select! içinde ödünç alındığı için yığın
    // taraması orada yapılamaz — aynı gerekçe, aynı desen.
    let mut pending_burst: Option<crossterm::event::KeyEvent> = None;
    // Teşhis (yalnızca TFS_KEYLOG ayarlıysa): girdi olaylarının zamanlaması.
    let mut keylog = KeyLog::new();

    loop {
        // --- Bekleyen kabuk durum geçişleri (select dışında) ---
        if pending_open {
            pending_open = false;
            let (rows, cols) = term_area(terminal)?;
            match TermSession::open(&ssh, rows, cols).await {
                Ok((t, rx)) => {
                    term = Some(t);
                    term_rx = Some(rx);
                }
                Err(e) => {
                    app.status = format!("Kabuk açılamadı: {e}");
                    screen = Screen::FileTransfer;
                }
            }
        }
        if shell_closed {
            shell_closed = false;
            term = None;
            term_rx = None;
            screen = Screen::FileTransfer;
            app.status = "Kabuk oturumu kapandı.".into();
        }

        // Yazılabilir tuş: önce hemen gönder (yazmaya gecikme eklenmesin),
        // sonra arkasından yığın gelip gelmediğine bak. Yığın varsa bu,
        // emülatörün tuş tuş enjekte ettiği bir yapıştırmadır → tek parça.
        if let Some(k) = pending_burst.take() {
            if let Some(t) = term.as_mut() {
                t.send_key(k).await;
                let rest = collect_key_burst(events.get()).await;
                if let Some(l) = keylog.as_mut() {
                    l.write(&format!("BURST {} karakter toplandı", rest.chars().count()));
                }
                if !rest.is_empty() {
                    t.send_burst(rest).await;
                }
            }
        }

        // Kabuktan biriken ek parçaları çiz-öncesi topluca işle (daha az redraw)
        // ve varsa terminal-sorgu yanıtını (CPR/DSR/DA) kabuğa gönder. `select!`
        // dışında olduğu için hem `term` hem `term_rx` aynı anda ödünç alınabilir.
        if let (Some(t), Some(rx)) = (term.as_mut(), term_rx.as_mut()) {
            while let Ok(more) = rx.try_recv() {
                t.feed(&more);
            }
            if let Some(reply) = t.take_reply() {
                t.send_bytes(reply).await;
            }
        }

        // --- Çizim ---
        match screen {
            Screen::FileTransfer => {
                terminal.draw(|f| ui::draw(f, app, &server_name))?;
            }
            Screen::Terminal => {
                if let Some(t) = term.as_mut() {
                    let (rows, cols) = term_area(terminal)?;
                    t.resize(rows, cols).await;
                }
                if let Some(t) = term.as_ref() {
                    terminal.draw(|f| terminal::draw(f, t, &server_name))?;
                }
            }
        }

        if app.should_quit {
            return Ok(());
        }

        // Bekleyen transfer (yalnızca dosya modu) — progress bar ile çalıştır.
        if let Some(req) = app.pending_transfer.take() {
            run_transfer(terminal, app, &ssh, req, &mut events, &server_name).await?;
            continue;
        }

        // Bekleyen düzenleme (F4) — editör TUI'yi askıya alıp geri getirir.
        if let Some(req) = app.pending_edit.take() {
            run_edit(terminal, app, &ssh, req, &mut events, &server_name).await?;
            continue;
        }

        // --- Olay bekleme: klavye/fare + kabuk çıktısı ---
        tokio::select! {
            maybe_ev = events.get().next() => {
                match maybe_ev {
                    Some(Ok(ev)) => {
                        if let Some(l) = keylog.as_mut() {
                            l.write(&format!("{ev:?}"));
                        }
                        // Global mod tuşları (F1/F2) — her iki modda yakalanır, kabuğa iletilmez.
                        if let Event::Key(k) = &ev {
                            if k.kind == KeyEventKind::Press {
                                match k.code {
                                    KeyCode::F(1) => {
                                        screen = Screen::Terminal;
                                        if term.is_none() {
                                            pending_open = true;
                                        }
                                        continue;
                                    }
                                    KeyCode::F(2) => {
                                        screen = Screen::FileTransfer;
                                        continue;
                                    }
                                    _ => {}
                                }
                            }
                        }

                        // Global fare: üst çubuktaki F1/F2 sekmelerine sol tık → mod değiştir.
                        if let Event::Mouse(m) = &ev {
                            if let MouseEventKind::Down(MouseButton::Left) = m.kind {
                                if let Some(tab) = terminal::hit_tab(m.column, m.row) {
                                    match tab {
                                        terminal::Tab::Terminal => {
                                            screen = Screen::Terminal;
                                            if term.is_none() {
                                                pending_open = true;
                                            }
                                        }
                                        terminal::Tab::Files => screen = Screen::FileTransfer,
                                    }
                                    continue;
                                }
                            }
                        }

                        match screen {
                            Screen::FileTransfer => {
                                if let Event::Resize(_, _) = ev {
                                    continue;
                                }
                                app.handle_event(ev, &ssh).await?;
                            }
                            Screen::Terminal => {
                                if let Some(t) = term.as_mut() {
                                    match ev {
                                        // Shift+Ins / Ctrl+Shift+V → panodan yapıştır
                                        // (kabuğa iletilmez).
                                        Event::Key(k)
                                            if k.kind == KeyEventKind::Press && is_paste_key(&k) =>
                                        {
                                            t.paste_from_clipboard().await;
                                        }
                                        // Shift+PgUp / Shift+PgDn → kaydırma tamponunda gez.
                                        Event::Key(k)
                                            if k.kind == KeyEventKind::Press
                                                && k.modifiers.contains(KeyModifiers::SHIFT)
                                                && matches!(
                                                    k.code,
                                                    KeyCode::PageUp | KeyCode::PageDown
                                                ) =>
                                        {
                                            let page = term_area(terminal)?.0 as usize;
                                            if k.code == KeyCode::PageUp {
                                                t.scroll_up(page);
                                            } else {
                                                t.scroll_down(page);
                                            }
                                        }
                                        // Yazılabilir tuş: devamında yığın olup
                                        // olmadığına döngü başında bakılır.
                                        Event::Key(k)
                                            if k.kind == KeyEventKind::Press
                                                && burst_char(&k).is_some() =>
                                        {
                                            pending_burst = Some(k);
                                            continue;
                                        }
                                        Event::Key(k) if k.kind == KeyEventKind::Press => {
                                            t.send_key(k).await;
                                        }
                                        // Yapıştırma (bracketed paste) → metni kabuğa yaz.
                                        Event::Paste(text) => {
                                            t.send_paste(text).await;
                                        }
                                        // Fareyle metin seçme (grid satırı = ekran satırı - 1,
                                        // üst çubuk için). Bırakınca panoya kopyalanır.
                                        // Sağ tık → panodan yapıştır (PuTTY alışkanlığı).
                                        Event::Mouse(m) => match m.kind {
                                            MouseEventKind::Down(MouseButton::Left) if m.row >= 1 => {
                                                t.sel_start(m.row - 1, m.column);
                                            }
                                            MouseEventKind::Drag(MouseButton::Left) if m.row >= 1 => {
                                                t.sel_update(m.row - 1, m.column);
                                            }
                                            MouseEventKind::Up(MouseButton::Left) => {
                                                t.sel_finish_copy();
                                            }
                                            MouseEventKind::Down(MouseButton::Right) => {
                                                t.paste_from_clipboard().await;
                                            }
                                            MouseEventKind::ScrollUp => {
                                                t.scroll_up(terminal::SCROLL_STEP);
                                            }
                                            MouseEventKind::ScrollDown => {
                                                t.scroll_down(terminal::SCROLL_STEP);
                                            }
                                            _ => {}
                                        },
                                        // Resize çizim aşamasında ele alınır.
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                    Some(Err(e)) => return Err(e.into()),
                    None => return Ok(()),
                }
            }

            // Kabuk çıktısı (yalnızca oturum açıkken). Alıcı yoksa branch pasif.
            maybe_bytes = async {
                match term_rx.as_mut() {
                    Some(rx) => rx.recv().await,
                    None => std::future::pending::<Option<Vec<u8>>>().await,
                }
            }, if term_rx.is_some() => {
                match maybe_bytes {
                    Some(bytes) => {
                        if let Some(t) = term.as_mut() {
                            t.feed(&bytes);
                        }
                    }
                    // Kanal kapandı (kullanıcı `exit` yazdı ya da bağlantı düştü).
                    None => shell_closed = true,
                }
            }
        }
    }
}

/// Terminal modunda panodan yapıştırma kısayolu mu? (Ctrl+V kabuğa gitmeli —
/// uzak programlarda Ctrl+V'nin kendi anlamı var — bu yüzden Shift şart.)
///
/// Not: Windows Terminal / conhost bu kısayolları çoğu zaman **kendisi** yakalar
/// ve panoyu tuş tuş enjekte eder; o durumda burası hiç çalışmaz, yığın toplama
/// (`drain_key_burst`) devreye girer.
fn is_paste_key(k: &crossterm::event::KeyEvent) -> bool {
    let shift = k.modifiers.contains(KeyModifiers::SHIFT);
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    match k.code {
        KeyCode::Insert => shift && !ctrl,
        KeyCode::Char('V') | KeyCode::Char('v') => ctrl && shift,
        _ => false,
    }
}

/// Yığın hâlinde gelebilecek "yazılabilir" tuşun ürettiği karakter.
/// Ctrl/Alt'lı tuşlar, ok tuşları vb. yığına dahil edilmez (yığını bitirirler).
fn burst_char(k: &crossterm::event::KeyEvent) -> Option<char> {
    if k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) {
        return None;
    }
    match k.code {
        KeyCode::Char(c) => Some(c),
        KeyCode::Enter => Some('\r'),
        KeyCode::Tab => Some('\t'),
        _ => None,
    }
}

/// Yığındaki tuşlar arası kabul edilen en büyük boşluk. Emülatörün enjekte
/// ettiği yapıştırma bazen tek seferde konsol tamponuna yazılır, bazen damla
/// damla gelir; bu eşik ikisini de toparlar. İnsan yazımında tuşlar arası
/// boşluk ≥60 ms, tuş tekrarında bile ≥32 ms (Windows'ta azami ~31/sn)
/// olduğundan sıradan yazma yığına dönüşmez.
const BURST_GAP: std::time::Duration = std::time::Duration::from_millis(25);

/// Tek seferde toplanacak en fazla bayt. Devasa bir yapıştırmada ekranın
/// donmuş görünmemesi için üst sınır; kalanı sonraki turda toplanır.
const BURST_MAX_BYTES: usize = 64 * 1024;

/// `TFS_KEYLOG=yol` ayarlıysa gelen her girdi olayını, bir öncekinden kaç ms
/// sonra geldiğiyle birlikte kaydeder. Yapıştırmanın nasıl teslim edildiğini
/// (tek seferde tamponlanmış mı, damla damla mı — ve hangi aralıkla) ölçmek
/// için: yığın eşiği `BURST_GAP` buna göre ayarlanır.
struct KeyLog {
    file: std::fs::File,
    last: std::time::Instant,
}

impl KeyLog {
    fn new() -> Option<Self> {
        let path = std::env::var("TFS_KEYLOG").ok()?;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .ok()?;
        Some(Self {
            file,
            last: std::time::Instant::now(),
        })
    }

    fn write(&mut self, what: &str) {
        use std::io::Write;
        let now = std::time::Instant::now();
        let dt = now.duration_since(self.last).as_micros();
        self.last = now;
        let _ = writeln!(self.file, "+{:>8} us  {what}", dt);
    }
}

/// Yazılabilir bir tuş **gönderildikten sonra** arkasından yığın gelip
/// gelmediğine bakar; gelen yazılabilir tuşları toplayıp döndürür.
///
/// Neden gerekli: Windows'ta emülatörün kendi yapıştırma kısayolu (Ctrl+V,
/// Shift+Ins, Yapıştır menüsü…) panodaki metni **tek tek tuş olayı** olarak
/// enjekte eder — crossterm'in `Event::Paste`'i Windows'ta hiç üretilmez,
/// çünkü eski konsol API'sinde bracketed paste yok. Toplamazsak yapıştırma
/// harf harf gider: her karakter ayrı bir SSH paketi (çok yavaş) ve `vim`
/// bunu yazım sanıp otomatik girinti uygular.
///
/// İlk karakter çağıran tarafından zaten gönderildiği için sıradan yazmaya
/// **hiç gecikme eklenmez**; buradaki bekleme yalnızca "arkasından bir şey
/// geliyor mu" dinlemesidir ve karakter zaten yoldayken ekrana da bir şey
/// düşmez.
///
/// (Akış üzerinden geneldir; böylece zamanlamaya duyarlı bu mantık gerçek
/// terminal olmadan test edilebiliyor.)
async fn collect_key_burst<S>(events: &mut futures::stream::Peekable<S>) -> String
where
    S: futures::Stream<Item = io::Result<Event>> + Unpin,
{
    use std::pin::Pin;

    /// Sıradaki olayla ne yapılacağı (peek borrow'unu bitirmek için ayrı adım).
    enum Step {
        Take(char),
        /// Tuş bırakma olayı: yığını bölmemeli, sessizce yutulur.
        Skip,
        Stop,
    }

    let mut text = String::new();
    while text.len() < BURST_MAX_BYTES {
        // NOT: burada `now_or_never()` **kullanılmamalı**. crossterm'in
        // `EventStream`'i Pending dönerken kendisini uyandıracak waker'ı
        // saklar; noop waker verilirse `stream_wake_task_executed` bayrağı
        // takılı kalır ve gerçek waker bir daha kaydedilemez — ana döngü
        // tuşlara sağır kalabilir. `timeout` her zaman gerçek waker'la yoklar.
        // Olay hazırsa zaten beklemeden döner.
        let peeked = match tokio::time::timeout(BURST_GAP, Pin::new(&mut *events).peek()).await {
            Ok(p) => p,
            // Sessizlik: yığın bitti (ya da hiç yoktu).
            Err(_) => break,
        };
        let step = match peeked {
            Some(Ok(Event::Key(k))) if k.kind == KeyEventKind::Press => match burst_char(k) {
                Some(c) => Step::Take(c),
                None => Step::Stop,
            },
            Some(Ok(Event::Key(_))) => Step::Skip,
            _ => Step::Stop,
        };
        match step {
            Step::Take(c) => {
                text.push(c);
                let _ = events.next().await;
            }
            Step::Skip => {
                let _ = events.next().await;
            }
            Step::Stop => break,
        }
    }
    text
}

/// Terminal çizim alanının (üst çubuk için -1 satır) satır/sütun boyutu.
fn term_area(terminal: &Terminal<CrosstermBackend<Stdout>>) -> Result<(u16, u16)> {
    let size = terminal.size()?;
    let rows = size.height.saturating_sub(1).max(1);
    let cols = size.width.max(1);
    Ok((rows, cols))
}

/// Transferi ayrı bir tokio task'inde çalıştırır; ilerlemeyi `mpsc` üzerinden
/// alıp progress bar'ı günceller. Bu sırada UI her güncellemede yeniden çizilir
/// (donma yok) ve `q`/`Esc` ile transfer iptal edilebilir.
async fn run_transfer(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
    ssh: &Arc<Ssh>,
    req: TransferRequest,
    events: &mut EventSource,
    server_name: &str,
) -> Result<bool> {
    let (tx, mut rx) = mpsc::unbounded_channel();
    // Çağıran (ör. `run_edit`) transferin gerçekten bittiğini bilmeli.
    let mut ok = false;

    let verb = match req.target {
        PanelId::Remote => "Yükleniyor",
        PanelId::Local => "İndiriliyor",
    };
    app.status = format!("{verb}: {} (q/Esc: iptal)", req.name);
    app.transfer = Some(TransferState {
        name: req.name.clone(),
        done: 0,
        total: 0,
    });

    // I/O ayrı task'te — ana döngüyü bloklamaz.
    let ssh_task = Arc::clone(ssh);
    let req_task = req.clone();
    let mut handle = tokio::spawn(async move {
        match (req_task.source, req_task.target) {
            (PanelId::Local, PanelId::Remote) => {
                ssh_task
                    .upload(&req_task.local_path, &req_task.remote_path, &tx)
                    .await
            }
            (PanelId::Remote, PanelId::Local) => {
                ssh_task
                    .download(&req_task.remote_path, &req_task.local_path, &tx)
                    .await
            }
            _ => Ok(()),
        }
    });

    loop {
        terminal.draw(|f| ui::draw(f, app, server_name))?;

        tokio::select! {
            // İlerleme güncellemesi
            Some(p) = rx.recv() => {
                if let Some(t) = app.transfer.as_mut() {
                    t.done = p.done;
                    t.total = p.total;
                }
            }
            // Transfer tamamlandı (ya da hata / iptal)
            res = &mut handle => {
                app.transfer = None;
                match res {
                    Ok(Ok(())) => {
                        ok = true;
                        app.status = format!("Tamamlandı: {}", req.name);
                        // Hedef paneli tazele.
                        match req.target {
                            PanelId::Remote => {
                                let cwd = app.remote.cwd.clone();
                                let _ = app.load_remote(ssh, cwd).await;
                            }
                            PanelId::Local => {
                                let cwd = std::path::PathBuf::from(&app.local.cwd);
                                let _ = app.load_local(cwd);
                            }
                        }
                    }
                    Ok(Err(e)) => app.status = format!("Transfer hatası: {e}"),
                    Err(e) if e.is_cancelled() => {} // iptal: durum zaten ayarlandı
                    Err(e) => app.status = format!("Transfer görevi hatası: {e}"),
                }
                break;
            }
            // Kullanıcı olayı: sadece iptal (q/Esc) dinle.
            maybe = events.get().next() => {
                if let Some(Ok(Event::Key(k))) = maybe {
                    if k.kind == KeyEventKind::Press
                        && matches!(k.code, KeyCode::Char('q') | KeyCode::Esc)
                    {
                        handle.abort();
                        app.transfer = None;
                        app.status = format!("Transfer iptal edildi: {}", req.name);
                        break;
                    }
                }
            }
        }
    }
    Ok(ok)
}

/// F4 akışı: dosyayı `fresh` editöründe açar.
///
/// Yerel dosya doğrudan açılır. Uzak dosya geçici bir dizine indirilir, editör
/// kapandıktan sonra **içeriği değiştiyse** geri yüklenir — böylece uzak
/// sunucuda editör kurulu olmasına gerek kalmaz.
///
/// Editör tam ekran çalıştığı için TUI askıya alınır; bu, `select!` içinde
/// yapılamaz (hem `terminal` hem `events` sahipliği gerekir), o yüzden
/// transferlerdeki gibi döngü başında, bayrak üzerinden çağrılır.
async fn run_edit(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
    ssh: &Arc<Ssh>,
    req: EditRequest,
    events: &mut EventSource,
    server_name: &str,
) -> Result<()> {
    // 1) Editörü bul; yoksa kurulumu teklif et.
    let bin = match editor::locate() {
        Some(b) => b,
        None => {
            if !confirm_install(terminal, app, events, server_name).await? {
                app.status = format!("Düzenleme iptal — {} kurulu değil.", editor::CRATE);
                return Ok(());
            }
            match editor::install(terminal, events).await? {
                editor::Installed::Ok(b) => {
                    app.status = format!("{} kuruldu.", editor::CRATE);
                    b
                }
                editor::Installed::Failed(why) => {
                    app.status = format!("Kurulum başarısız: {why}");
                    return Ok(());
                }
            }
        }
    };

    match req.panel {
        // Yerel dosya: kopya yok, doğrudan aç.
        PanelId::Local => {
            // Liste içeriği değişmedi (dosya adı/varlığı aynı), bu yüzden
            // paneli yenilemiyoruz — yenileme kullanıcının imlecini sıfırlardı.
            let err = editor::open(terminal, events, &bin, &req.local_path)
                .await
                .err();
            app.status = match err {
                Some(e) => format!("Editör hatası: {e}"),
                None => format!("Düzenlendi: {}", req.name),
            };
        }

        // Uzak dosya: indir → düzenle → (değiştiyse) geri yükle.
        PanelId::Remote => {
            let size = ssh
                .sftp
                .metadata(&req.remote_path)
                .await
                .ok()
                .and_then(|m| m.size)
                .unwrap_or(0);
            if size > editor::MAX_EDIT_BYTES {
                app.status = format!(
                    "{} çok büyük ({} MiB) — düzenleme için sınır {} MiB.",
                    req.name,
                    size / (1024 * 1024),
                    editor::MAX_EDIT_BYTES / (1024 * 1024)
                );
                return Ok(());
            }

            // Geçici dizin açılamazsa (disk dolu, /tmp salt-okunur…) uygulamayı
            // düşürmek yerine durum çubuğunda söyle.
            let tmp = match editor::temp_file_for(&req.name) {
                Ok(t) => t,
                Err(e) => {
                    app.status = format!("Geçici dosya açılamadı: {e}");
                    return Ok(());
                }
            };

            let download = TransferRequest {
                source: PanelId::Remote,
                target: PanelId::Local,
                name: req.name.clone(),
                local_path: tmp.clone(),
                remote_path: req.remote_path.clone(),
            };
            // `run_transfer` hedef paneli (burada YEREL) tazeler ve seçimi
            // sıfırlar; oysa indirdiğimiz yer geçici dizin — kullanıcının yerel
            // paneldeki imleci yerinde kalmalı.
            let local_sel = app.selected_name(PanelId::Local);
            let ok = run_transfer(terminal, app, ssh, download, events, server_name).await?;
            if let Some(name) = &local_sel {
                app.select_by_name(PanelId::Local, name);
            }
            if !ok {
                editor::cleanup_temp(&tmp);
                return Ok(());
            }

            // Editör dosyaya hiç dokunmamış olabilir (ya da kaydedip aynı içeriği
            // yazmış olabilir); gereksiz yüklemeyi önlemek için içerik özeti.
            let before = editor::content_hash(&tmp).ok();
            let err = editor::open(terminal, events, &bin, &tmp).await.err();
            let after = editor::content_hash(&tmp).ok();

            // Özet alınamadıysa (ör. editör dosyayı sildi) yüklemeyi deneme.
            if after.is_none() {
                editor::cleanup_temp(&tmp);
                app.status = format!("{} okunamadı — yükleme yapılmadı.", req.name);
                return Ok(());
            }
            if before == after {
                editor::cleanup_temp(&tmp);
                app.status = match err {
                    Some(e) => format!("Editör hatası: {e}"),
                    None => format!("{} değişmedi — yükleme yapılmadı.", req.name),
                };
                return Ok(());
            }

            let upload = TransferRequest {
                source: PanelId::Local,
                target: PanelId::Remote,
                name: req.name.clone(),
                local_path: tmp.clone(),
                remote_path: req.remote_path.clone(),
            };
            if run_transfer(terminal, app, ssh, upload, events, server_name).await? {
                editor::cleanup_temp(&tmp);
                app.select_by_name(PanelId::Remote, &req.name);
                app.status = format!("✓ {} kaydedildi ve sunucuya yüklendi.", req.name);
            } else {
                // Geçici dosya BİLEREK silinmiyor: kullanıcının emeği burada.
                app.status = format!(
                    "Yükleme başarısız — değişiklikleriniz burada: {}",
                    tmp.display()
                );
            }
        }
    }
    Ok(())
}

/// "fresh kurulu değil, kurulsun mu?" onayı. Mevcut dosya ekranının üzerine
/// bir kutu çizip yalnızca evet/hayır tuşlarını dinler.
async fn confirm_install(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
    events: &mut EventSource,
    server_name: &str,
) -> Result<bool> {
    let plan = editor::install_plan();
    loop {
        terminal.draw(|f| {
            ui::draw(f, app, server_name);
            editor::draw_install_prompt(f, &plan);
        })?;

        match events.get().next().await {
            Some(Ok(Event::Key(k))) if k.kind == KeyEventKind::Press => match k.code {
                // cargo yoksa kurulacak bir şey de yok; yalnızca kapatılır.
                KeyCode::Char('e') | KeyCode::Char('E') | KeyCode::Enter if !plan.is_empty() => {
                    return Ok(true)
                }
                KeyCode::Char('h')
                | KeyCode::Char('H')
                | KeyCode::Char('q')
                | KeyCode::Enter
                | KeyCode::Esc => return Ok(false),
                _ => {}
            },
            Some(Ok(_)) => {}
            Some(Err(e)) => return Err(e.into()),
            None => return Ok(false),
        }
    }
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    enter_tui(&mut terminal)?;
    Ok(terminal)
}

/// TUI kipine gir(ilir): ham mod + alternatif ekran + fare + bracketed paste.
/// Açılışta ve harici bir editörden döndükten sonra kullanılır; `clear()`
/// ratatui'nin önceki kare önbelleğini geçersiz kılar, böylece alt sürecin
/// ekranda bıraktığı hiçbir şey kalmaz.
pub(crate) fn enter_tui(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    enable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste
    )?;
    terminal.clear()?;
    Ok(())
}

pub(crate) fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        DisableBracketedPaste,
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    Ok(())
}

/// Yapıştırma yığını toplama testleri. Bu mantık **her tuş vuruşunun** yolunda
/// olduğu için (yanlışlıkla yazımı yığın sanırsa yazmak bozulur) zamanlamaya
/// duyarlı kısmı burada gerçek terminal olmadan doğrulanıyor.
#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEvent;
    use futures::stream::{self, StreamExt};

    fn press(code: KeyCode) -> io::Result<Event> {
        Ok(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)))
    }

    fn ch(c: char) -> io::Result<Event> {
        press(KeyCode::Char(c))
    }

    /// Akış biter (peek → None) — zamanlayıcıya hiç girilmez.
    fn closing(evs: Vec<io::Result<Event>>) -> futures::stream::Peekable<stream::Iter<std::vec::IntoIter<io::Result<Event>>>> {
        stream::iter(evs).peekable()
    }

    /// Tuşun ardından hiçbir şey gelmezse yığın yok — sıradan yazım.
    #[tokio::test]
    async fn arkasi_bos_ise_yigin_yok() {
        let mut s = closing(vec![]);
        assert_eq!(collect_key_burst(&mut s).await, "");
    }

    #[tokio::test]
    async fn ardisik_karakterler_tek_parca_olur() {
        let mut s = closing(vec![ch('e'), ch('l'), ch('l'), ch('o')]);
        assert_eq!(collect_key_burst(&mut s).await, "ello");
    }

    /// Çok satırlı yapıştırma: Enter `\r`'a çevrilir ve yığını bölmez.
    #[tokio::test]
    async fn enter_ve_tab_yigina_dahil() {
        let mut s = closing(vec![ch('a'), press(KeyCode::Enter), press(KeyCode::Tab), ch('b')]);
        assert_eq!(collect_key_burst(&mut s).await, "a\r\tb");
    }

    /// Ok tuşu gibi yazılabilir olmayan bir tuş yığını bitirir ve **tüketilmez**
    /// (ana döngü onu normal tuş olarak işlemeye devam edebilmeli).
    #[tokio::test]
    async fn yazilabilir_olmayan_tus_yigini_bitirir_ve_akista_kalir() {
        let mut s = closing(vec![ch('a'), press(KeyCode::Left), ch('b')]);
        assert_eq!(collect_key_burst(&mut s).await, "a");
        let next = s.next().await.unwrap().unwrap();
        assert_eq!(next, Event::Key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE)));
    }

    /// Ctrl'lü tuş (yapıştırılan metinde olamaz) yığını bitirir.
    #[tokio::test]
    async fn ctrl_tusu_yigini_bitirir() {
        let ctrl_c = Ok(Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)));
        let mut s = closing(vec![ch('a'), ctrl_c]);
        assert_eq!(collect_key_burst(&mut s).await, "a");
    }

    /// Windows'ta tuş bırakma olayları araya girebilir; yığını bölmemeli.
    #[tokio::test]
    async fn tus_birakma_olaylari_yutulur() {
        let release = Ok(Event::Key(KeyEvent::new_with_kind(
            KeyCode::Char('a'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        )));
        let mut s = closing(vec![ch('a'), release, ch('b')]);
        assert_eq!(collect_key_burst(&mut s).await, "ab");
    }

    /// Fare/resize gibi tuş olmayan olay yığını bitirir (akışta kalır).
    #[tokio::test]
    async fn tus_disi_olay_yigini_bitirir() {
        let mut s = closing(vec![ch('a'), Ok(Event::Resize(80, 24))]);
        assert_eq!(collect_key_burst(&mut s).await, "a");
    }

    /// Akış açık ama boş kalırsa sonsuza kadar beklememeli.
    /// `start_paused` sanal zamanı ilerletir.
    #[tokio::test(start_paused = true)]
    async fn acik_ama_bos_akista_takilmaz() {
        let mut s = stream::iter(vec![ch('a')]).chain(stream::pending()).peekable();
        assert_eq!(collect_key_burst(&mut s).await, "a");
    }

    /// Sıradan yazım (tuşlar arası uzun boşluk) yığın sayılmamalı.
    #[tokio::test(start_paused = true)]
    async fn yavas_yazim_yigin_sayilmaz() {
        // `Box::pin`: `once(async …)` Unpin değil.
        let slow = Box::pin(stream::once(async {
            tokio::time::sleep(std::time::Duration::from_millis(80)).await;
            ch('b')
        }));
        let mut s = slow.peekable();
        assert_eq!(collect_key_burst(&mut s).await, "");
    }

    /// Damla damla gelen (emülatörün yavaş enjekte ettiği) yapıştırma da
    /// toparlanmalı: 10 ms aralıklı karakterler tek yığın olur.
    #[tokio::test(start_paused = true)]
    async fn damla_damla_gelen_yapistirma_toparlanir() {
        let trickle = Box::pin(stream::unfold(0usize, |i| async move {
            if i >= 4 {
                return None;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            Some((ch("abcd".as_bytes()[i] as char), i + 1))
        }));
        let mut s = trickle.peekable();
        assert_eq!(collect_key_burst(&mut s).await, "abcd");
    }
}
