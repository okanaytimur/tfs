//! tfs (terminal-file-send) — SSH dosya tarayıcısı (F2) + modern SSH terminali (F1).
//!
//! Sunucular `config.json` dosyasından okunur (birden fazla sunucu, parolalarıyla).
//! Açılışta fareyle sunucu seçilir. Config yolu ilk argümanla değiştirilebilir:
//!   cargo run                 # ./config.json
//!   cargo run -- sunucular.json

mod app;
mod config;
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
        Event, EventStream, KeyCode, KeyEventKind, MouseButton, MouseEventKind,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures::StreamExt;
use ratatui::{backend::CrosstermBackend, Terminal};
use tokio::sync::mpsc;

use crate::app::{App, PanelId, TransferRequest, TransferState};
use crate::config::Config;
use crate::ssh::Ssh;
use crate::terminal::TermSession;

/// Aktif ekran (mod). F1 → Terminal, F2 → Dosya transferi.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    FileTransfer,
    Terminal,
}

#[tokio::main]
async fn main() -> Result<()> {
    let config_path = std::env::args().nth(1).unwrap_or_else(|| "config.json".into());
    let cfg = Config::load(&config_path)?;

    let mut terminal = setup_terminal()?;
    // TUI içindeki tüm akışı sar; hata olsa da terminali geri yükle.
    let res = app_flow(&mut terminal, &cfg).await;
    restore_terminal(&mut terminal)?;
    res
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
    let mut events = EventStream::new();
    let mut screen = Screen::FileTransfer;

    // Kabuk oturumu ilk F1'de tembel açılır, sonra uygulama boyunca canlı tutulur.
    // Okuma yarısının alıcısı `term_rx`'te; `select!` bunu dinler.
    let mut term: Option<TermSession> = None;
    let mut term_rx: Option<mpsc::UnboundedReceiver<Vec<u8>>> = None;

    // Durum geçişleri `select!` içinde borrow çakışmasına yol açmasın diye
    // bayrakla işaretlenip döngü başında (select dışında) uygulanır.
    let mut pending_open = false;
    let mut shell_closed = false;

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

        // --- Olay bekleme: klavye/fare + kabuk çıktısı ---
        tokio::select! {
            maybe_ev = events.next() => {
                match maybe_ev {
                    Some(Ok(ev)) => {
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
                                        Event::Key(k) if k.kind == KeyEventKind::Press => {
                                            t.send_key(k).await;
                                        }
                                        // Yapıştırma (bracketed paste) → metni kabuğa yaz.
                                        Event::Paste(text) => {
                                            t.send_bytes(text.into_bytes()).await;
                                        }
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
    events: &mut EventStream,
    server_name: &str,
) -> Result<()> {
    let (tx, mut rx) = mpsc::unbounded_channel();

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
            maybe = events.next() => {
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
    Ok(())
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste
    )?;
    let backend = CrosstermBackend::new(stdout);
    Ok(Terminal::new(backend)?)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
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
