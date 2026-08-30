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
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};
use russh::{client, ChannelMsg, ChannelWriteHalf};
use tokio::sync::mpsc;
use tui_term::widget::{Cursor, PseudoTerminal};

/// Kaydırma (scrollback) tampon satır sayısı.
const SCROLLBACK: usize = 1000;

/// Fare tekerleği / Shift+PgUp bir adımda kaç satır kaydırır.
pub const SCROLL_STEP: usize = 3;

/// Üst çubuktaki sekme etiketleri (fare hit-test'i ile senkron kalması için sabit).
const TAB_F1: &str = " F1 Terminal ";
const TAB_GAP: &str = " ";
const TAB_F2: &str = " F2 Dosya ";

/// Üst çubuktaki tıklanabilir sekmeler.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Terminal,
    Files,
}

/// (col,row) üst çubuktaki bir sekmeye denk geliyorsa hangisi? Çubuk her zaman
/// 0. satırda ve x=0'dan başlar (hem dosya hem terminal modunda aynı düzen).
pub fn hit_tab(col: u16, row: u16) -> Option<Tab> {
    if row != 0 {
        return None;
    }
    let f1_w = TAB_F1.chars().count() as u16;
    let gap_w = TAB_GAP.chars().count() as u16;
    let f2_w = TAB_F2.chars().count() as u16;
    let f2_start = f1_w + gap_w;
    if col < f1_w {
        Some(Tab::Terminal)
    } else if col >= f2_start && col < f2_start + f2_w {
        Some(Tab::Files)
    } else {
        None
    }
}

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
    /// Fareyle metin seçimi (grid koordinatları: (satır, sütun)).
    selection: Option<Selection>,
    /// Sistem panosu (kopyalama için). Erişilemezse `None`.
    clipboard: Option<arboard::Clipboard>,
    /// Kopyalama sonrası üst çubukta gösterilecek kısa bilgi (bir sonraki tuşa
    /// kadar durur).
    copy_note: Option<String>,
    /// Okuma task'i; drop edilince iptal olur.
    _reader: tokio::task::JoinHandle<()>,
}

/// Fareyle sürüklenen metin seçimi. Koordinatlar grid'e göre (satır, sütun).
#[derive(Clone, Copy)]
struct Selection {
    anchor: (u16, u16),
    head: (u16, u16),
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
            selection: None,
            clipboard: arboard::Clipboard::new().ok(),
            copy_note: None,
            _reader: reader,
        };
        Ok((session, rx))
    }

    // --- Fareyle metin seçimi + kopyalama ---

    /// Seçimi başlat (fare sol tuş basıldı). `row`/`col` grid koordinatı.
    pub fn sel_start(&mut self, row: u16, col: u16) {
        let p = self.clamp(row, col);
        self.selection = Some(Selection { anchor: p, head: p });
        self.copy_note = None;
    }

    /// Seçimi güncelle (fare sürüklendi).
    pub fn sel_update(&mut self, row: u16, col: u16) {
        let p = self.clamp(row, col);
        if let Some(s) = &mut self.selection {
            s.head = p;
        }
    }

    /// Seçimi bitir (fare bırakıldı) ve seçili metni panoya kopyala. Sürükleme
    /// olmadıysa (tek tık) seçim temizlenir, kopyalama yapılmaz.
    pub fn sel_finish_copy(&mut self) {
        let Some(s) = self.selection else {
            return;
        };
        if s.anchor == s.head {
            self.selection = None;
            return;
        }
        let text = self.selected_text();
        if !text.is_empty() {
            let n = text.chars().count();
            if let Some(cb) = self.clipboard.as_mut() {
                if cb.set_text(text).is_ok() {
                    self.copy_note = Some(format!("✓ {n} karakter kopyalandı"));
                } else {
                    self.copy_note = Some("⚠ pano erişilemedi".into());
                }
            } else {
                self.copy_note = Some("⚠ pano yok".into());
            }
        }
    }

    /// Seçimi ve kopyalama bilgisini temizle (ör. tuşa basınca).
    pub fn clear_selection(&mut self) {
        self.selection = None;
        self.copy_note = None;
    }

    /// Panodaki metni kabuğa yapıştırır (sağ tık / Shift+Ins). Pano boşsa ya da
    /// erişilemiyorsa üst çubukta kısa bir uyarı gösterilir.
    pub async fn paste_from_clipboard(&mut self) {
        let text = match self.clipboard.as_mut() {
            Some(cb) => match cb.get_text() {
                Ok(t) => t,
                Err(_) => {
                    self.copy_note = Some("⚠ pano boş".into());
                    return;
                }
            },
            None => {
                self.copy_note = Some("⚠ pano yok".into());
                return;
            }
        };
        if text.is_empty() {
            self.copy_note = Some("⚠ pano boş".into());
            return;
        }
        self.send_paste(text).await;
    }

    /// Arka arkaya gelen tuşlardan toplanan yığını tek parça gönderir
    /// (bkz. `main::collect_key_burst`).
    ///
    /// Yalnızca **çok satırlı** yığın bracketed paste ile sarılır: tek satırlık
    /// bir yığını tek tek göndermekle tek parça göndermek arasında uzak taraf
    /// için hiçbir fark yoktur, ama sarmak fark yaratır — `vim` normal modunda
    /// bracketed paste "metni yapıştır" demektir, dolayısıyla tuşu basılı
    /// tutunca (tuş tekrarı) yanlışlıkla yığın sanılırsa `jjjj` imleci
    /// indirmek yerine metin olarak yapışırdı. Satır sonu içeren yığın ise
    /// gerçekten yapıştırmadır ve sarılması şart (otomatik girinti bozulmasın,
    /// kabukta satırlar kendiliğinden çalışmasın).
    pub async fn send_burst(&mut self, text: String) {
        if text.contains('\r') || text.contains('\n') {
            self.send_paste(text).await;
        } else {
            self.clear_selection();
            self.scroll_reset();
            let _ = self.writer.data_bytes(text.into_bytes()).await;
        }
    }

    /// Metni yapıştırma olarak gönderir. Satır sonları kabuğun beklediği `\r`'a
    /// çevrilir; uzak taraf bracketed paste modundaysa metin `ESC[200~`/`ESC[201~`
    /// ile sarılır (böylece `vim`/`bash` yapıştırmayı yazımdan ayırt eder).
    pub async fn send_paste(&mut self, text: String) {
        self.clear_selection();
        self.scroll_reset();
        let body = text.replace("\r\n", "\r").replace('\n', "\r");
        let bytes = if self.parser.screen().bracketed_paste() {
            let mut b = Vec::with_capacity(body.len() + 12);
            b.extend_from_slice(b"\x1b[200~");
            b.extend_from_slice(body.as_bytes());
            b.extend_from_slice(b"\x1b[201~");
            b
        } else {
            body.into_bytes()
        };
        let _ = self.writer.data_bytes(bytes).await;
    }

    // --- Kaydırma tamponu (scrollback) ---

    /// Geçmişe doğru kaydır (fare tekerleği yukarı / Shift+PgUp).
    pub fn scroll_up(&mut self, lines: usize) {
        let target = self.parser.screen().scrollback() + lines;
        self.set_scroll(target);
    }

    /// Şimdiki zamana doğru kaydır (fare tekerleği aşağı / Shift+PgDn).
    pub fn scroll_down(&mut self, lines: usize) {
        let target = self.parser.screen().scrollback().saturating_sub(lines);
        self.set_scroll(target);
    }

    /// Kaydırmayı en alta (canlı ekrana) döndür. Zaten alttaysa iş yapmaz.
    pub fn scroll_reset(&mut self) {
        if self.parser.screen().scrollback() != 0 {
            self.set_scroll(0);
        }
    }

    /// Kaydırma konumunu ayarlar. Seçim koordinatları kayacağı için seçim
    /// temizlenir. `set_scrollback` istenen değeri tampon uzunluğuna kırpar.
    fn set_scroll(&mut self, target: usize) {
        self.parser.set_scrollback(target);
        self.selection = None;
        self.copy_note = None;
    }

    /// Kaç satır geride olduğumuz (0 = canlı ekran).
    pub fn scrollback(&self) -> usize {
        self.parser.screen().scrollback()
    }

    fn clamp(&self, row: u16, col: u16) -> (u16, u16) {
        (
            row.min(self.rows.saturating_sub(1)),
            col.min(self.cols.saturating_sub(1)),
        )
    }

    /// Normalize edilmiş seçim aralığı: (baş_satır, baş_sütun, son_satır,
    /// son_sütun_dahil). Sürükleme yoksa `None`.
    fn selection_span(&self) -> Option<(u16, u16, u16, u16)> {
        let s = self.selection?;
        if s.anchor == s.head {
            return None;
        }
        let (start, end) = if s.anchor <= s.head {
            (s.anchor, s.head)
        } else {
            (s.head, s.anchor)
        };
        let end_col_incl = (end.1 + 1).min(self.cols);
        Some((start.0, start.1, end.0, end_col_incl))
    }

    fn selected_text(&self) -> String {
        match self.selection_span() {
            Some((sr, sc, er, ec)) => self.parser.screen().contents_between(sr, sc, er, ec),
            None => String::new(),
        }
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

    /// Bir tuşu ANSI baytlarına kodlayıp kabuğa yazar. Yazmaya başlayınca varsa
    /// seçim vurgusu temizlenir ve (gerçek terminaller gibi) ekran canlı görünüme
    /// döner.
    pub async fn send_key(&mut self, key: KeyEvent) {
        self.clear_selection();
        self.scroll_reset();
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

    // Üst çubuk notu: kopyalama bilgisi ya da (kaydırma varsa) konum göstergesi.
    let scrolled = sess.scrollback();
    let scroll_note;
    let note = if let Some(n) = sess.copy_note.as_deref() {
        Some(n)
    } else if scrolled > 0 {
        scroll_note = format!("⇡ {scrolled} satır geride (Enter/yazınca canlıya döner)");
        Some(scroll_note.as_str())
    } else {
        None
    };
    draw_top_bar(f, root[0], server_name, true, sess.closed, note);

    // Geçmişe bakarken imleci gizle — canlı ekranda değil, kaydırılmış içerikte
    // duruyormuş gibi görünmesin.
    let mut pt = PseudoTerminal::new(sess.screen());
    if scrolled > 0 {
        pt = pt.cursor(Cursor::default().visibility(false));
    }
    f.render_widget(pt, root[1]);

    // Seçim vurgusu: sözde-terminal çizildikten sonra buffer üzerine REVERSED uygula.
    if let Some((sr, sc, er, ec)) = sess.selection_span() {
        highlight_selection(f, root[1], sr, sc, er, ec);
    }
}

/// Grid koordinatlı seçim aralığını (satır bazlı, okuma sırasına göre) çizim
/// alanı üzerinde ters renkle (REVERSED) vurgular.
fn highlight_selection(f: &mut Frame, area: Rect, sr: u16, sc: u16, er: u16, ec: u16) {
    let cols = area.width;
    let buf = f.buffer_mut();
    for r in sr..=er {
        if r >= area.height {
            break;
        }
        let (c0, c1) = if sr == er {
            (sc, ec)
        } else if r == sr {
            (sc, cols)
        } else if r == er {
            (0, ec)
        } else {
            (0, cols)
        };
        for c in c0..c1.min(cols) {
            if let Some(cell) = buf.cell_mut((area.x + c, area.y + r)) {
                cell.set_style(Style::default().add_modifier(Modifier::REVERSED));
            }
        }
    }
}

/// Üst mod/başlık çubuğu. Her iki modda da kullanılır (`terminal_active` hangi
/// modun vurgulanacağını belirler).
pub fn draw_top_bar(
    f: &mut Frame,
    area: Rect,
    server_name: &str,
    terminal_active: bool,
    closed: bool,
    note: Option<&str>,
) {
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
        Span::styled(TAB_F1, term_style),
        Span::raw(TAB_GAP),
        Span::styled(TAB_F2, files_style),
        Span::raw("  "),
        Span::styled(
            format!("⛁ {server_name}"),
            Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD),
        ),
    ];
    if let Some(n) = note {
        // Kopyalama gibi anlık bir bilgi varsa onu göster (ipucunun yerine).
        spans.push(Span::styled(
            format!("  {n}"),
            Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD),
        ));
    } else if terminal_active {
        let hint = if closed {
            "  — kabuk kapandı (F1: yeniden aç)"
        } else {
            "  — sürükle: kopyala · sağ tık/Shift+Ins: yapıştır · tekerlek: geçmiş · çıkış: exit"
        };
        spans.push(Span::styled(hint, Style::default().fg(Color::DarkGray)));
    } else {
        spans.push(Span::styled(
            "  — sürükle-bırak: transfer · F4: fresh ile düzenle · q: çıkış",
            Style::default().fg(Color::DarkGray),
        ));
    }

    let bar = Paragraph::new(Line::from(spans)).style(Style::default().bg(Color::Reset));
    f.render_widget(bar, area);
}
