//! F1 — modern SSH terminali (PTY + VT100 emülasyonu).
//!
//! Sunucudan gelen ANSI bayt akışı `vt100::Parser` ile bir ekran ızgarasına
//! çevrilir ve `tui-term`'in `PseudoTerminal` widget'ı ile çizilir. Kullanıcı
//! tuşları ANSI baytlarına kodlanıp (`encode_key`) kabuk kanalına yazılır.
//!
//! Okuma ayrı bir tokio task'inde yapılır; gelen baytlar `mpsc` ile ana döngüye
//! taşınır (UI bloklanmaz). Yazma yarısı (`ChannelWriteHalf`) ana döngüde tutulur.

use anyhow::{Context, Result};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};
use russh::{client, ChannelMsg, ChannelWriteHalf};
use tokio::sync::mpsc;
use tui_term::widget::PseudoTerminal;

/// Kaydırma (scrollback) tampon satır sayısı.
const SCROLLBACK: usize = 1000;

/// Interaktif kabuk oturumu: yazma yarısı + VT100 parser + boyut bilgisi.
/// Okuma yarısı arka plandaki bir task'te; baytlar `open()`'ın döndürdüğü
/// alıcı (receiver) üzerinden ana döngüye akar.
pub struct TermSession {
    writer: ChannelWriteHalf<client::Msg>,
    parser: vt100::Parser,
    rows: u16,
    cols: u16,
    /// Kabuk kapandı mı (sunucu EOF/Close gönderdi ya da kanal düştü).
    pub closed: bool,
    /// Sunucunun terminal sorgularına (CPR/DSR/DA) verilecek, gönderilmeyi
    /// bekleyen yanıt baytları.
    pending_reply: Vec<u8>,
    /// Teşhis için: `TFS_LOG` ortam değişkeni ayarlıysa gelen ham baytların
    /// escape'lenmiş logu.
    log: Option<std::fs::File>,
    /// Okuma task'i; drop edilince iptal olur.
    _reader: tokio::task::JoinHandle<()>,
}

impl TermSession {
    /// Aynı SSH bağlantısı üzerinde kabuk açar. Okuma task'ini başlatır ve
    /// (oturum, bayt-alıcısı) döndürür. Alıcı ana döngünün `select!`'inde dinlenir.
    pub async fn open(
        ssh: &crate::ssh::Ssh,
        rows: u16,
        cols: u16,
    ) -> Result<(Self, mpsc::UnboundedReceiver<Vec<u8>>)> {
        let channel = ssh
            .open_shell(cols, rows)
            .await
            .context("kabuk oturumu açılamadı")?;
        let (mut read, writer) = channel.split();

        let (tx, rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let reader = tokio::spawn(async move {
            loop {
                let data = match read.wait().await {
                    // stdout ve stderr aynı akışa (terminale) yazılır.
                    Some(ChannelMsg::Data { data })
                    | Some(ChannelMsg::ExtendedData { data, .. }) => data,
                    // EOF / Close / kanal düştü → oku task'i biter, tx düşer.
                    Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => break,
                    _ => continue,
                };
                if tx.send(data.to_vec()).is_err() {
                    break;
                }
            }
        });

        // Teşhis logu (opsiyonel): TFS_LOG=yol ayarlıysa gelen baytlar loglanır.
        let log = std::env::var("TFS_LOG").ok().and_then(|path| {
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .ok()
        });

        let session = Self {
            writer,
            parser: vt100::Parser::new(rows, cols, SCROLLBACK),
            rows,
            cols,
            closed: false,
            pending_reply: Vec::new(),
            log,
            _reader: reader,
        };
        Ok((session, rx))
    }

    /// Sunucudan gelen ham baytları VT100 parser'a besler ve terminal
    /// sorgularına (CPR/DSR/DA) yanıt hazırlar.
    pub fn feed(&mut self, bytes: &[u8]) {
        if let Some(f) = self.log.as_mut() {
            use std::io::Write;
            let _ = writeln!(f, "{}", bytes.escape_ascii());
        }
        self.parser.process(bytes);
        scan_queries(bytes, self.parser.screen(), &mut self.pending_reply);
    }

    /// Bekleyen terminal-sorgu yanıtını (varsa) alır; ana döngü bunu kabuğa yazar.
    pub fn take_reply(&mut self) -> Option<Vec<u8>> {
        if self.pending_reply.is_empty() {
            None
        } else {
            Some(std::mem::take(&mut self.pending_reply))
        }
    }

    pub fn screen(&self) -> &vt100::Screen {
        self.parser.screen()
    }

    /// Terminal alanı değiştiyse parser boyutunu güncelle ve sunucuya
    /// `window_change` gönder.
    pub async fn resize(&mut self, rows: u16, cols: u16) {
        if rows == 0 || cols == 0 || (rows == self.rows && cols == self.cols) {
            return;
        }
        self.rows = rows;
        self.cols = cols;
        self.parser.set_size(rows, cols);
        let _ = self
            .writer
            .window_change(cols as u32, rows as u32, 0, 0)
            .await;
    }

    /// Bir tuşu ANSI baytlarına kodlayıp kabuğa yazar.
    pub async fn send_key(&mut self, key: KeyEvent) {
        if let Some(bytes) = encode_key(&key) {
            let _ = self.writer.data_bytes(bytes).await;
        }
    }

    /// Ham baytları (ör. yapıştırma ya da terminal-sorgu yanıtı) kabuğa yazar.
    pub async fn send_bytes(&mut self, bytes: Vec<u8>) {
        let _ = self.writer.data_bytes(bytes).await;
    }
}

/// Gelen bayt akışında terminal sorgularını tarar ve yanıtlarını `out`'a ekler.
/// Gerçek bir terminal emülatörü bunlara yanıt vermezse bazı kabuklar/promptlar
/// ~1 sn timeout bekleyip ekranı sıfırlar — bu yüzden şart.
///
/// Ele alınanlar:
/// - `ESC [ 6 n` (CPR — imleç konumu) → `ESC [ satır ; sütun R` (1-tabanlı)
/// - `ESC [ 5 n` (DSR — durum) → `ESC [ 0 n` (OK)
/// - `ESC [ c` / `ESC [ 0 c` (Primary DA) → `ESC [ ? 1 ; 2 c` (VT100)
///
/// Not: sorgu iki ayrı parçaya bölünürse bu basit tarama kaçırabilir (nadir).
fn scan_queries(bytes: &[u8], screen: &vt100::Screen, out: &mut Vec<u8>) {
    let mut i = 0;
    while i + 1 < bytes.len() {
        // CSI: ESC [
        if bytes[i] == 0x1b && bytes[i + 1] == b'[' {
            let params_start = i + 2;
            let mut j = params_start;
            while j < bytes.len()
                && (bytes[j].is_ascii_digit()
                    || bytes[j] == b';'
                    || bytes[j] == b'?'
                    || bytes[j] == b'>')
            {
                j += 1;
            }
            if j < bytes.len() {
                let params = &bytes[params_start..j];
                match bytes[j] {
                    b'n' if params == b"6" => {
                        // vt100: cursor_position() -> (satır, sütun), 0-tabanlı.
                        let (row, col) = screen.cursor_position();
                        out.extend_from_slice(
                            format!("\x1b[{};{}R", row + 1, col + 1).as_bytes(),
                        );
                    }
                    b'n' if params == b"5" => out.extend_from_slice(b"\x1b[0n"),
                    b'c' if params.is_empty() || params == b"0" => {
                        out.extend_from_slice(b"\x1b[?1;2c")
                    }
                    _ => {}
                }
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
}

/// crossterm `KeyEvent` → kabuğa gönderilecek ANSI baytları.
/// `None` = iletilmeyecek tuş.
fn encode_key(key: &KeyEvent) -> Option<Vec<u8>> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);

    let mut out: Vec<u8> = match key.code {
        KeyCode::Char(c) => {
            if ctrl {
                // Ctrl+harf → kontrol baytı (ör. Ctrl+C = 0x03, Ctrl+@ = 0x00).
                let up = c.to_ascii_uppercase() as u32;
                if (0x40..0x80).contains(&up) {
                    vec![(up as u8) & 0x1f]
                } else {
                    return None;
                }
            } else {
                let mut buf = [0u8; 4];
                c.encode_utf8(&mut buf).as_bytes().to_vec()
            }
        }
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Esc => vec![0x1b],
        KeyCode::Up => b"\x1b[A".to_vec(),
        KeyCode::Down => b"\x1b[B".to_vec(),
        KeyCode::Right => b"\x1b[C".to_vec(),
        KeyCode::Left => b"\x1b[D".to_vec(),
        KeyCode::Home => b"\x1b[H".to_vec(),
        KeyCode::End => b"\x1b[F".to_vec(),
        KeyCode::PageUp => b"\x1b[5~".to_vec(),
        KeyCode::PageDown => b"\x1b[6~".to_vec(),
        KeyCode::Insert => b"\x1b[2~".to_vec(),
        KeyCode::Delete => b"\x1b[3~".to_vec(),
        KeyCode::F(n) => match n {
            // F1/F2 uygulama-global mod tuşları — kabuğa iletilmez (main.rs yakalar).
            3 => b"\x1bOR".to_vec(),
            4 => b"\x1bOS".to_vec(),
            5 => b"\x1b[15~".to_vec(),
            6 => b"\x1b[17~".to_vec(),
            7 => b"\x1b[18~".to_vec(),
            8 => b"\x1b[19~".to_vec(),
            9 => b"\x1b[20~".to_vec(),
            10 => b"\x1b[21~".to_vec(),
            11 => b"\x1b[23~".to_vec(),
            12 => b"\x1b[24~".to_vec(),
            _ => return None,
        },
        _ => return None,
    };

    // Alt (Meta) → dizinin başına ESC eklenir.
    if alt {
        let mut prefixed = Vec::with_capacity(out.len() + 1);
        prefixed.push(0x1b);
        prefixed.append(&mut out);
        out = prefixed;
    }
    Some(out)
}

/// Terminal modunu çizer: üst mod/başlık çubuğu + tam ekran sözde-terminal.
pub fn draw(f: &mut Frame, sess: &TermSession, server_name: &str) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(f.area());

    draw_top_bar(f, root[0], server_name, true, sess.closed);

    let pt = PseudoTerminal::new(sess.screen());
    f.render_widget(pt, root[1]);
}

/// Üst mod/başlık çubuğu. Her iki modda da kullanılır (`terminal_active` hangi
/// modun vurgulanacağını belirler).
pub fn draw_top_bar(f: &mut Frame, area: ratatui::layout::Rect, server_name: &str, terminal_active: bool, closed: bool) {
    let active = Style::default()
        .fg(Color::Black)
        .bg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let inactive = Style::default().fg(Color::Gray).bg(Color::DarkGray);

    let (term_style, files_style) = if terminal_active {
        (active, inactive)
    } else {
        (inactive, active)
    };

    let mut spans = vec![
        Span::styled(" F1 Terminal ", term_style),
        Span::raw(" "),
        Span::styled(" F2 Dosya ", files_style),
        Span::raw("  "),
        Span::styled(
            format!("⛁ {server_name}"),
            Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD),
        ),
    ];
    if terminal_active {
        let hint = if closed {
            "  — kabuk kapandı (F1: yeniden aç)"
        } else {
            "  — çıkış: 'exit' ya da F2 → q"
        };
        spans.push(Span::styled(hint, Style::default().fg(Color::DarkGray)));
    } else {
        spans.push(Span::styled(
            "  — q: çıkış",
            Style::default().fg(Color::DarkGray),
        ));
    }

    let bar = Paragraph::new(Line::from(spans)).style(Style::default().bg(Color::Reset));
    f.render_widget(bar, area);
}
