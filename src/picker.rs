//! Açılış ekranı: **bağlantı yöneticisi**.
//!
//! `config.json`'daki sunucuları listeler, fareyle seçtirir ve aynı ekranda
//! ekle / düzenle / kopyala / sil / sırala işlemlerini yaptırır. Her değişiklik
//! anında `config.json`'a yazılır (bkz. `Config::save`) — ayrı bir "kaydet"
//! adımı yok, ama form açıkken hiçbir şey yazılmaz: yazma yalnızca formu
//! onaylayınca olur.
//!
//! `ssh-list`ten farkı: bu araç parolayı da saklayabiliyor. tfs zaten tek
//! bağlantıyı iki kanalda (terminal + SFTP) kendi kütüphanesiyle kuruyor,
//! harici `ssh` istemcisine devretmiyor; parolayı sormak için her seferinde
//! kullanıcıyı beklemek yerine config'de tutma seçeneği veriliyor. Parola düz
//! metin saklandığı için dosya Unix'te `0600`'e çekiliyor ve arayüzde
//! maskeleniyor (F9 ile görünür).

use std::io::Stdout;

use anyhow::Result;
use crossterm::event::{
    Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use futures::StreamExt;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
    Frame, Terminal,
};

use crate::config::{Config, ServerConfig};

type Term = Terminal<CrosstermBackend<Stdout>>;

/// Araç çubuğundaki (ve kısayollardaki) eylemler.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    New,
    Edit,
    Copy,
    Delete,
    MoveUp,
    MoveDown,
}

/// Araç çubuğu düğmeleri: etiket, kısayol adı, eylem. Çizim ve tıklama
/// eşlemesi tek kaynaktan beslensin diye burada duruyor.
const BUTTONS: [(&str, &str, Action); 6] = [
    ("+ Yeni", "F5", Action::New),
    ("Düzenle", "F4", Action::Edit),
    ("Kopyala", "F6", Action::Copy),
    ("Sil", "F8", Action::Delete),
    ("↑", "Alt+↑", Action::MoveUp),
    ("↓", "Alt+↓", Action::MoveDown),
];

/// Ekranın hangi katmanı açık.
enum Mode {
    /// Liste (varsayılan).
    List,
    /// Düzenleme formu açık. `Box` — form `Mode`un diğer varyantlarından
    /// çok daha büyük, kutulanmazsa her `Mode` değeri o boyu taşır.
    Form(Box<Form>),
    /// "Silinsin mi?" onayı; içindeki indeks `servers` içindeki mutlak konum.
    Confirm { index: usize, name: String },
}

/// Bağlantı yöneticisinin tüm durumu.
struct Manager<'a> {
    servers: &'a mut Vec<ServerConfig>,
    /// Değişiklikler buraya yazılır.
    path: &'a str,
    /// Sorguya uyan sunucuların `servers` içindeki indeksleri.
    view: Vec<usize>,
    query: String,
    /// `view` içindeki konum (mutlak indeks değil).
    selected: usize,
    offset: usize,
    list_area: Rect,
    /// En son çizimde düğmelerin nereye düştüğü (fare eşlemesi için).
    buttons: Vec<(Rect, Action)>,
    status: String,
    mode: Mode,
    /// `true` olunca `run` cikar (kullanici vazgecti).
    quit: bool,
}

/// Sunucu listesini gösterir; seçilen sunucunun `servers` içindeki indeksini
/// döndürür. `None` = kullanıcı çıktı.
///
/// `servers` **değiştirilebilir**: kullanıcı bu ekranda bağlantı ekleyip
/// silebildiği için liste (ve dolayısıyla indeksler) çağrı sırasında değişir.
pub async fn run(
    term: &mut Term,
    servers: &mut Vec<ServerConfig>,
    config_path: &str,
) -> Result<Option<usize>> {
    let bos = servers.is_empty();
    let mut m = Manager {
        servers,
        path: config_path,
        view: Vec::new(),
        query: String::new(),
        selected: 0,
        offset: 0,
        list_area: Rect::default(),
        buttons: Vec::new(),
        status: if bos {
            format!("Henüz bağlantı yok — [+ Yeni] (F5) ile ekleyin. Dosya: {config_path}")
        } else {
            format!("Enter / tıkla: bağlan · yaz: ara · Esc: çık · {config_path}")
        },
        mode: Mode::List,
        quit: false,
    };
    m.refilter();

    let mut events = EventStream::new();

    loop {
        term.draw(|f| m.draw(f))?;

        match events.next().await {
            Some(Ok(Event::Key(k))) if k.kind == KeyEventKind::Press => {
                if let Some(i) = m.on_key(k) {
                    return Ok(Some(i));
                }
                if m.quit {
                    return Ok(None);
                }
            }
            Some(Ok(Event::Mouse(mo))) => {
                if let Some(i) = m.on_mouse(mo) {
                    return Ok(Some(i));
                }
            }
            Some(Ok(_)) => {}
            Some(Err(e)) => return Err(e.into()),
            None => return Ok(None),
        }
    }
}

impl Manager<'_> {
    /// Seçili sunucunun `servers` içindeki mutlak indeksi.
    fn current(&self) -> Option<usize> {
        self.view.get(self.selected).copied()
    }

    /// `view`i sorguya göre yeniden kurar; seçim hâlâ görünüyorsa üstünde kalır
    /// (harf sildikçe imleç zıplamasın — `app::Panel::refilter` ile aynı fikir).
    fn refilter(&mut self) {
        let onceki = self.current();
        let q = self.query.to_lowercase();
        self.view = self
            .servers
            .iter()
            .enumerate()
            .filter(|(_, s)| q.is_empty() || eslesir(s, &q))
            .map(|(i, _)| i)
            .collect();
        self.selected = onceki
            .and_then(|i| self.view.iter().position(|&v| v == i))
            .unwrap_or(0)
            .min(self.view.len().saturating_sub(1));
        self.clamp_scroll();
    }

    fn clamp_scroll(&mut self) {
        let h = (self.list_area.height as usize).max(1);
        if self.selected < self.offset {
            self.offset = self.selected;
        } else if self.selected >= self.offset + h {
            self.offset = self.selected + 1 - h;
        }
        let max_offset = self.view.len().saturating_sub(h);
        self.offset = self.offset.min(max_offset);
    }

    /// Seçimi `view` içinde taşır.
    fn select(&mut self, idx: usize) {
        self.selected = idx.min(self.view.len().saturating_sub(1));
        self.clamp_scroll();
    }

    /// Listeyi diske yazar. Her mutasyondan sonra çağrılır: kullanıcı ayrı bir
    /// "kaydet" düğmesi aramasın, çıkarken de bir şey kaybolmasın.
    fn persist(&mut self) {
        let cfg = Config {
            servers: self.servers.clone(),
        };
        self.status = match cfg.save(self.path) {
            Ok(()) => format!("Kaydedildi → {}", self.path),
            Err(e) => format!("KAYDEDİLEMEDİ: {e:#}"),
        };
    }

    // ---------------------------------------------------------------- eylemler

    /// Bir araç çubuğu eylemini uygular — hem düğme tıklaması hem kısayol
    /// buraya iner, davranış tek yerde dursun.
    fn act(&mut self, a: Action) {
        match a {
            Action::New => {
                self.mode = Mode::Form(Box::new(Form::new(None, &ServerConfig::default())));
            }
            Action::Edit => match self.current() {
                Some(i) => {
                    self.mode = Mode::Form(Box::new(Form::new(Some(i), &self.servers[i])))
                }
                None => self.status = "Düzenlenecek bağlantı yok.".into(),
            },
            Action::Copy => match self.current() {
                Some(i) => {
                    let mut kopya = self.servers[i].clone();
                    kopya.name = format!("{} (kopya)", kopya.name);
                    self.servers.insert(i + 1, kopya);
                    self.query.clear();
                    self.refilter();
                    if let Some(pos) = self.view.iter().position(|&v| v == i + 1) {
                        self.select(pos);
                    }
                    self.persist();
                }
                None => self.status = "Kopyalanacak bağlantı yok.".into(),
            },
            Action::Delete => match self.current() {
                Some(i) => {
                    self.mode = Mode::Confirm {
                        index: i,
                        name: self.servers[i].name.clone(),
                    }
                }
                None => self.status = "Silinecek bağlantı yok.".into(),
            },
            Action::MoveUp | Action::MoveDown => self.reorder(a == Action::MoveUp),
        }
    }

    /// Seçili bağlantıyı listede bir yukarı/aşağı taşır.
    ///
    /// Filtre açıkken kapalıdır: görünen komşu ile gerçek komşu farklı olduğu
    /// için sürükleme sonucu kullanıcının gördüğüyle uyuşmaz.
    fn reorder(&mut self, yukari: bool) {
        if !self.query.is_empty() {
            self.status = "Sıralama için önce aramayı temizleyin (Esc).".into();
            return;
        }
        let Some(i) = self.current() else { return };
        let j = if yukari {
            if i == 0 {
                return;
            }
            i - 1
        } else {
            if i + 1 >= self.servers.len() {
                return;
            }
            i + 1
        };
        self.servers.swap(i, j);
        self.refilter();
        self.select(j);
        self.persist();
    }

    /// Formu onaylar: doğrula, listeye yaz, diske kaydet.
    fn commit(&mut self, form: Form) {
        let sc = match form.build() {
            Ok(sc) => sc,
            Err(e) => {
                // Hata formun içinde gösterilir, kullanıcı alanda kalır.
                let mut f = form;
                f.error = Some(e);
                self.mode = Mode::Form(Box::new(f));
                return;
            }
        };
        let idx = match form.target {
            Some(i) => {
                self.servers[i] = sc;
                i
            }
            None => {
                self.servers.push(sc);
                self.servers.len() - 1
            }
        };
        self.mode = Mode::List;
        self.refilter();
        // Yeni/düzenlenen kayıt filtreye takılmış olabilir; görünmüyorsa
        // aramayı temizleyip üstüne git — kullanıcı az önce yazdığını görsün.
        if !self.view.contains(&idx) {
            self.query.clear();
            self.refilter();
        }
        if let Some(pos) = self.view.iter().position(|&v| v == idx) {
            self.select(pos);
        }
        self.persist();
    }

    // ----------------------------------------------------------------- girdiler

    /// Tuş olayı. Dönüş `Some(i)` = bu sunucuya bağlan.
    fn on_key(&mut self, k: KeyEvent) -> Option<usize> {
        if matches!(self.mode, Mode::Form(_)) {
            self.form_key(k);
            return None;
        }
        // İndeksi önce dışarı al: arm içinde `self.mode` değişecek.
        let onay = match &self.mode {
            Mode::Confirm { index, .. } => Some(*index),
            _ => None,
        };
        if let Some(index) = onay {
            match k.code {
                KeyCode::Char('e') | KeyCode::Char('E') | KeyCode::Enter => {
                    self.servers.remove(index);
                    self.mode = Mode::List;
                    self.refilter();
                    self.persist();
                }
                _ => self.mode = Mode::List,
            }
            return None;
        }
        self.list_key(k)
    }

    fn list_key(&mut self, k: KeyEvent) -> Option<usize> {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let alt = k.modifiers.contains(KeyModifiers::ALT);

        match k.code {
            // Çıkış — app.rs ile aynı kurallar: Ctrl+Q / F10, Esc önce aramayı
            // temizler.
            KeyCode::Char('q') | KeyCode::Char('Q') if ctrl => self.quit = true,
            KeyCode::F(10) => self.quit = true,
            KeyCode::Esc => {
                if self.query.is_empty() {
                    self.quit = true;
                } else {
                    self.query.clear();
                    self.refilter();
                }
            }

            KeyCode::Enter => return self.current(),

            KeyCode::F(5) | KeyCode::Insert => self.act(Action::New),
            KeyCode::F(4) => self.act(Action::Edit),
            KeyCode::F(6) => self.act(Action::Copy),
            KeyCode::F(8) | KeyCode::Delete => self.act(Action::Delete),
            KeyCode::Up if alt => self.act(Action::MoveUp),
            KeyCode::Down if alt => self.act(Action::MoveDown),

            KeyCode::Backspace => {
                self.query.pop();
                self.refilter();
            }
            // Yazılabilir karakter → arama. Ctrl/Alt'lılar kısayol olabilir.
            KeyCode::Char(c) if !ctrl && !alt => {
                self.query.push(c);
                self.refilter();
            }

            KeyCode::Down => self.select(self.selected + 1),
            KeyCode::Up => self.select(self.selected.saturating_sub(1)),
            KeyCode::Home => self.select(0),
            KeyCode::End => self.select(self.view.len().saturating_sub(1)),
            KeyCode::PageDown => {
                let page = (self.list_area.height as usize).max(1);
                self.select(self.selected + page);
            }
            KeyCode::PageUp => {
                let page = (self.list_area.height as usize).max(1);
                self.select(self.selected.saturating_sub(page));
            }
            _ => {}
        }
        None
    }

    fn form_key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let shift = k.modifiers.contains(KeyModifiers::SHIFT);

        // Formu kapatan tuşlar önce gelir: `self.mode`a dokundukları için
        // formu ödünç almadan ele alınmalılar.
        match k.code {
            KeyCode::Esc => {
                self.mode = Mode::List;
                return;
            }
            KeyCode::Enter | KeyCode::F(10) => {
                self.commit_form();
                return;
            }
            KeyCode::Char('s') if ctrl => {
                self.commit_form();
                return;
            }
            _ => {}
        }

        let Mode::Form(f) = &mut self.mode else {
            return;
        };
        match k.code {
            KeyCode::F(9) => f.show_secrets = !f.show_secrets,
            KeyCode::Tab => {
                if shift {
                    f.prev_field()
                } else {
                    f.next_field()
                }
            }
            KeyCode::BackTab => f.prev_field(),
            KeyCode::Down => f.next_field(),
            KeyCode::Up => f.prev_field(),
            KeyCode::Left => f.cursor = f.cursor.saturating_sub(1),
            KeyCode::Right => f.cursor = (f.cursor + 1).min(f.value().chars().count()),
            KeyCode::Home => f.cursor = 0,
            KeyCode::End => f.cursor = f.value().chars().count(),
            KeyCode::Backspace => f.backspace(),
            KeyCode::Delete => f.delete(),
            KeyCode::Char(c) if !ctrl => f.insert(c),
            _ => {}
        }
    }

    /// Açık formu moddan çıkarıp onaylar.
    fn commit_form(&mut self) {
        let Mode::Form(f) = std::mem::replace(&mut self.mode, Mode::List) else {
            return;
        };
        self.commit(*f);
    }

    /// Fare olayı. Dönüş `Some(i)` = bu sunucuya bağlan.
    fn on_mouse(&mut self, m: MouseEvent) -> Option<usize> {
        match &self.mode {
            Mode::Confirm { .. } => return None,
            Mode::Form(_) => {
                if m.kind == MouseEventKind::Down(MouseButton::Left) {
                    self.form_click(m.column, m.row);
                }
                return None;
            }
            Mode::List => {}
        }

        // Araç çubuğu düğmeleri.
        if m.kind == MouseEventKind::Down(MouseButton::Left) {
            if let Some(a) = self
                .buttons
                .iter()
                .find(|(r, _)| icinde(*r, m.column, m.row))
                .map(|(_, a)| *a)
            {
                self.act(a);
                return None;
            }
        }

        let idx = self.hit(m.column, m.row)?;
        match m.kind {
            // Tek tık: seç + bağlan (fare öncelikli — düzenleme araç
            // çubuğundan yapılır, listeye tıklamak hep "bağlan" demektir).
            MouseEventKind::Down(MouseButton::Left) => return self.view.get(idx).copied(),
            MouseEventKind::Moved => self.select(idx),
            MouseEventKind::ScrollDown => self.select(self.selected + 1),
            MouseEventKind::ScrollUp => self.select(self.selected.saturating_sub(1)),
            _ => {}
        }
        None
    }

    fn form_click(&mut self, col: u16, row: u16) {
        // Yerleşim `Rect`leri `Copy` — önce kopyala ki `self.mode`u değiştiren
        // dallar formu hâlâ ödünç almış olmasın.
        let Mode::Form(f) = &self.mode else { return };
        let (kaydet, vazgec, goster, alanlar) =
            (f.save_area, f.cancel_area, f.reveal_area, f.field_areas);

        if icinde(kaydet, col, row) {
            self.commit_form();
            return;
        }
        if icinde(vazgec, col, row) {
            self.mode = Mode::List;
            return;
        }

        let Mode::Form(f) = &mut self.mode else { return };
        if icinde(goster, col, row) {
            f.show_secrets = !f.show_secrets;
            return;
        }
        // Alana tıklama: o alana odaklan, imleci tıklanan sütuna koy.
        for (i, r) in alanlar.iter().enumerate() {
            if icinde(*r, col, row) {
                f.cur = i;
                let rel = (col - r.x) as usize + f.scroll;
                f.cursor = rel.min(f.fields[i].chars().count());
                return;
            }
        }
    }

    /// (col,row) liste alanında mı? İse `view` içinde hangi konum.
    fn hit(&self, col: u16, row: u16) -> Option<usize> {
        if !icinde(self.list_area, col, row) {
            return None;
        }
        let idx = self.offset + (row - self.list_area.y) as usize;
        (idx < self.view.len()).then_some(idx)
    }

    // -------------------------------------------------------------------- çizim

    fn draw(&mut self, f: &mut Frame) {
        let root = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),    // liste + ayrıntı
                Constraint::Length(1), // araç çubuğu
                Constraint::Length(1), // durum
            ])
            .split(f.area());

        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(root[0]);

        self.draw_list(f, cols[0]);
        self.draw_details(f, cols[1]);
        self.draw_toolbar(f, root[1]);

        let durum = if self.query.is_empty() {
            format!(" {} ", self.status)
        } else {
            format!(" ara: {}   ({} eşleşme) ", self.query, self.view.len())
        };
        f.render_widget(
            Paragraph::new(Span::styled(
                durum,
                Style::default().fg(Color::Black).bg(Color::Cyan),
            )),
            root[2],
        );

        match &mut self.mode {
            // Form yerleşimi (alan + düğme `Rect`leri) çizim sırasında
            // hesaplanıp forma yazılır; fare eşlemesi onları okur.
            Mode::Form(form) => {
                form.layout_and_render(f);
                form.place_cursor(f);
            }
            Mode::Confirm { name, .. } => draw_confirm(f, name),
            Mode::List => {}
        }
    }

    fn draw_list(&mut self, f: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title(format!(" SSH Bağlantıları ({}) ", self.servers.len()));
        self.list_area = block.inner(area);
        self.clamp_scroll();

        if self.view.is_empty() {
            let mesaj = if self.servers.is_empty() {
                "Henüz bağlantı yok.\n\n[+ Yeni] (F5) ile ilk bağlantınızı ekleyin."
            } else {
                "Aramaya uyan bağlantı yok.\n\n(Esc: aramayı temizle)"
            };
            f.render_widget(
                Paragraph::new(mesaj)
                    .alignment(Alignment::Center)
                    .style(Style::default().fg(Color::DarkGray))
                    .block(block),
                area,
            );
            return;
        }

        // Ad sütunu sabit genişlikte: adresler alt alta hizalansın. Genişlik
        // en uzun ada göre, ama liste alanının yarısını geçmeden — uzun tek bir
        // ad yüzünden adres sütunu ezilmesin.
        let ad_w = self
            .servers
            .iter()
            .map(|s| s.name.chars().count())
            .max()
            .unwrap_or(0)
            .min((self.list_area.width as usize / 2).max(8));

        let h = self.list_area.height as usize;
        let items: Vec<ListItem> = self.view[self.offset..(self.offset + h).min(self.view.len())]
            .iter()
            .map(|&i| {
                let s = &self.servers[i];
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!(" {:<ad_w$}  ", kirp(&s.name, ad_w)),
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("{}@{}:{}", s.user, s.host, s.port),
                        Style::default().fg(Color::Gray),
                    ),
                ]))
            })
            .collect();

        let list = List::new(items)
            .block(block)
            .highlight_symbol("▶")
            .highlight_style(
                Style::default()
                    .bg(Color::Blue)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            );
        let mut state = ListState::default();
        state.select(Some(self.selected.saturating_sub(self.offset)));
        f.render_stateful_widget(list, area, &mut state);
    }

    fn draw_details(&self, f: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(" Ayrıntı ");

        let Some(s) = self.current().and_then(|i| self.servers.get(i)) else {
            f.render_widget(block, area);
            return;
        };

        let etiket = Style::default().fg(Color::DarkGray);
        let deger = Style::default().fg(Color::White);
        let mut satirlar = vec![
            alan("Ad", &s.name, etiket, deger),
            alan("Sunucu", &format!("{}:{}", s.host, s.port), etiket, deger),
            alan("Kullanıcı", &s.user, etiket, deger),
            Line::from(""),
        ];

        // Kimlik doğrulama özeti — `ssh::Ssh::connect` sırası: önce anahtar,
        // sonra parola. Kullanıcı hangisinin devrede olduğunu buradan görsün.
        satirlar.push(Line::from(Span::styled("Kimlik doğrulama", etiket)));
        satirlar.push(match s.key.as_deref().filter(|k| !k.is_empty()) {
            Some(k) => alan("  Anahtar", k, etiket, Style::default().fg(Color::LightGreen)),
            None => alan(
                "  Anahtar",
                "varsayılanlar (id_ed25519 · id_ecdsa · id_rsa)",
                etiket,
                Style::default().fg(Color::DarkGray),
            ),
        });
        satirlar.push(if s.password.is_empty() {
            alan("  Parola", "— yok —", etiket, Style::default().fg(Color::DarkGray))
        } else {
            alan(
                "  Parola",
                &"•".repeat(s.password.chars().count().min(24)),
                etiket,
                Style::default().fg(Color::LightGreen),
            )
        });
        if s.key_passphrase.as_deref().is_some_and(|p| !p.is_empty()) {
            satirlar.push(alan(
                "  Anh. parolası",
                "•••• (kayıtlı)",
                etiket,
                Style::default().fg(Color::LightGreen),
            ));
        }

        satirlar.push(Line::from(""));
        satirlar.push(Line::from(Span::styled(
            "Enter / tıkla → bağlan",
            Style::default().fg(Color::Cyan),
        )));

        f.render_widget(Paragraph::new(satirlar).block(block), area);
    }

    fn draw_toolbar(&mut self, f: &mut Frame, area: Rect) {
        self.buttons.clear();
        // Çok kısa ekranda `Layout` bu satıra sıfır yükseklik verebilir; her
        // düğme `height: 1` ile çizildiği için burada durmazsak tamponun
        // dışına yazarız.
        if area.height == 0 || area.width == 0 {
            return;
        }
        let mut x = area.x;
        for (etiket, kisayol, act) in BUTTONS {
            let metin = format!(" {etiket} ({kisayol}) ");
            let w = metin.chars().count() as u16;
            if x + w > area.x + area.width {
                break;
            }
            let r = Rect {
                x,
                y: area.y,
                width: w,
                height: 1,
            };
            f.render_widget(
                Paragraph::new(Span::styled(
                    metin,
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Gray)
                        .add_modifier(Modifier::BOLD),
                )),
                r,
            );
            self.buttons.push((r, act));
            x += w + 1;
        }
        // Kalan yere ipucu koymuyoruz: 6 düğme 80 sütunun çoğunu yiyor, ipucu
        // yarısından kesiliyordu. Aynı bilgi durum satırında, tam hâliyle.
    }
}

/// `Ad : değer` satırı.
fn alan(etiket: &str, deger: &str, e: Style, d: Style) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{etiket:<15}"), e),
        Span::styled(deger.to_string(), d),
    ])
}

/// Sunucu aramaya uyuyor mu? Ad, host ve kullanıcı birlikte taranır — kullanıcı
/// "root" ya da "10.0" yazınca da bulsun.
fn eslesir(s: &ServerConfig, q: &str) -> bool {
    s.name.to_lowercase().contains(q)
        || s.host.to_lowercase().contains(q)
        || s.user.to_lowercase().contains(q)
}

/// (col,row) `r`nin içinde mi? Genişliği/yüksekliği sıfır olan `Rect` (ekrana
/// sığmadığı için çizilmemiş düğme) hiçbir noktayı içermez.
/// Metni en fazla `n` karaktere indirger (taşarsa sonuna `…`).
fn kirp(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    s.chars().take(n.saturating_sub(1)).chain(['…']).collect()
}

fn icinde(r: Rect, col: u16, row: u16) -> bool {
    col >= r.x && col < r.x + r.width && row >= r.y && row < r.y + r.height
}

// ============================================================ düzenleme formu

/// Form alanları: etiket + gizli mi (maskelensin mi).
const FIELDS: [(&str, bool); 7] = [
    ("Ad", false),
    ("Sunucu (host)", false),
    ("Port", false),
    ("Kullanıcı", false),
    ("Parola", true),
    ("Anahtar dosyası", false),
    ("Anahtar parolası", true),
];

const F_NAME: usize = 0;
const F_HOST: usize = 1;
const F_PORT: usize = 2;
const F_USER: usize = 3;
const F_PASS: usize = 4;
const F_KEY: usize = 5;
const F_KEYPASS: usize = 6;

/// Tek bir bağlantının düzenleme formu.
struct Form {
    /// `None` = yeni bağlantı, `Some(i)` = `servers[i]` düzenleniyor.
    target: Option<usize>,
    fields: [String; FIELDS.len()],
    /// Odaklı alan.
    cur: usize,
    /// İmlecin odaklı alandaki konumu (**karakter** indeksi, bayt değil).
    cursor: usize,
    /// Değer kutusundan taşan metinde yatay kaydırma (karakter cinsinden).
    scroll: usize,
    /// Parolalar açık gösteriliyor mu (F9).
    show_secrets: bool,
    error: Option<String>,
    field_areas: [Rect; FIELDS.len()],
    save_area: Rect,
    cancel_area: Rect,
    reveal_area: Rect,
    /// İmlecin çizileceği ekran konumu (çizim sırasında hesaplanır).
    cursor_at: Option<(u16, u16)>,
}

impl Form {
    fn new(target: Option<usize>, s: &ServerConfig) -> Self {
        let fields = [
            s.name.clone(),
            s.host.clone(),
            s.port.to_string(),
            s.user.clone(),
            s.password.clone(),
            s.key.clone().unwrap_or_default(),
            s.key_passphrase.clone().unwrap_or_default(),
        ];
        let cursor = fields[0].chars().count();
        Self {
            target,
            fields,
            cur: 0,
            cursor,
            scroll: 0,
            show_secrets: false,
            error: None,
            field_areas: [Rect::default(); FIELDS.len()],
            save_area: Rect::default(),
            cancel_area: Rect::default(),
            reveal_area: Rect::default(),
            cursor_at: None,
        }
    }

    fn value(&self) -> &str {
        &self.fields[self.cur]
    }

    fn next_field(&mut self) {
        self.cur = (self.cur + 1) % FIELDS.len();
        self.cursor = self.value().chars().count();
    }

    fn prev_field(&mut self) {
        self.cur = (self.cur + FIELDS.len() - 1) % FIELDS.len();
        self.cursor = self.value().chars().count();
    }

    /// İmleçten önceki karakteri siler.
    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let b = bayt(&self.fields[self.cur], self.cursor - 1);
        self.fields[self.cur].remove(b);
        self.cursor -= 1;
    }

    /// İmlecin üstündeki karakteri siler.
    fn delete(&mut self) {
        let s = &mut self.fields[self.cur];
        if self.cursor >= s.chars().count() {
            return;
        }
        let b = bayt(s, self.cursor);
        s.remove(b);
    }

    fn insert(&mut self, c: char) {
        // Port alanı yalnızca rakam kabul eder; harf yazmak sessizce yutulur
        // (kaydederken hata vermektense girişte engellemek daha az sürpriz).
        if self.cur == F_PORT && !c.is_ascii_digit() {
            return;
        }
        let b = bayt(&self.fields[self.cur], self.cursor);
        self.fields[self.cur].insert(b, c);
        self.cursor += 1;
    }

    /// Alanlardan `ServerConfig` üretir; doğrulama hatası `Err(mesaj)`.
    fn build(&self) -> Result<ServerConfig, String> {
        let host = self.fields[F_HOST].trim().to_string();
        if host.is_empty() {
            return Err("Sunucu (host) boş olamaz.".into());
        }
        let user = self.fields[F_USER].trim().to_string();
        if user.is_empty() {
            return Err("Kullanıcı adı boş olamaz.".into());
        }
        let port_txt = self.fields[F_PORT].trim();
        let port: u16 = if port_txt.is_empty() {
            22
        } else {
            port_txt
                .parse()
                .map_err(|_| "Port 1–65535 arasında bir sayı olmalı.".to_string())?
        };
        if port == 0 {
            return Err("Port 1–65535 arasında bir sayı olmalı.".into());
        }
        // Ad boşsa host'u kullan — listede adsız satır görünmesin.
        let name = match self.fields[F_NAME].trim() {
            "" => host.clone(),
            n => n.to_string(),
        };

        let bos_none = |s: &str| {
            let t = s.trim();
            (!t.is_empty()).then(|| t.to_string())
        };

        Ok(ServerConfig {
            name,
            host,
            port,
            user,
            // Parolalar trim'lenmez: baştaki/sondaki boşluk parolanın parçası
            // olabilir.
            password: self.fields[F_PASS].clone(),
            key: bos_none(&self.fields[F_KEY]),
            key_passphrase: (!self.fields[F_KEYPASS].is_empty())
                .then(|| self.fields[F_KEYPASS].clone()),
        })
    }

    fn place_cursor(&mut self, f: &mut Frame) {
        if let Some((x, y)) = self.cursor_at {
            f.set_cursor_position((x, y));
        }
    }

    fn layout_and_render(&mut self, f: &mut Frame) {
        let baslik = match self.target {
            Some(_) => " Bağlantıyı düzenle ",
            None => " Yeni bağlantı ",
        };

        // 7 alan + boşluk + hata + boşluk + düğmeler = 11 iç satır. `ortala`
        // ekrana sığmıyorsa kırpar; o yüzden aşağıdaki her satır `inner`ın
        // içinde mi diye ayrıca bakılıyor — `Frame::render_widget` alanı
        // kırpmaz, tamponun dışına taşan `Rect` panikler.
        let area = ortala(f.area(), 68, FIELDS.len() as u16 + 6);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            .title(baslik);
        let inner = block.inner(area);
        f.render_widget(Clear, area);
        f.render_widget(block, area);

        // Dar ekranda etiket sütunu da kısalır; değer kutusuna en az 1 sütun
        // kalsın diye etiket `inner`ın yarısını geçmiyor.
        let etiket_w = 18.min(inner.width / 2);
        let deger_x = inner.x + etiket_w;
        let deger_w = inner.width.saturating_sub(etiket_w);

        for (i, (etiket, gizli)) in FIELDS.iter().enumerate() {
            let y = inner.y + i as u16;
            if y >= inner.y + inner.height || deger_w == 0 {
                self.field_areas[i] = Rect::default();
                continue;
            }
            let odakli = i == self.cur;

            f.render_widget(
                Paragraph::new(Span::styled(
                    format!("{etiket:<w$}", w = etiket_w as usize),
                    Style::default().fg(if odakli { Color::Yellow } else { Color::Gray }),
                )),
                Rect { x: inner.x, y, width: etiket_w, height: 1 },
            );

            let kutu = Rect { x: deger_x, y, width: deger_w, height: 1 };
            self.field_areas[i] = kutu;

            // Gösterilecek metin: gizli alanlar maskelenir.
            let ham = &self.fields[i];
            let gosterilen: String = if *gizli && !self.show_secrets {
                "•".repeat(ham.chars().count())
            } else {
                ham.clone()
            };

            // Odaklı alanda imleç görünür kalacak şekilde yatay kaydır.
            let genislik = deger_w.saturating_sub(1) as usize;
            let kaydir = if odakli {
                let k = self.cursor.saturating_sub(genislik);
                self.scroll = k;
                k
            } else {
                0
            };
            let parca: String = gosterilen.chars().skip(kaydir).take(genislik + 1).collect();

            f.render_widget(
                Paragraph::new(Span::styled(
                    format!("{parca:<w$}", w = deger_w as usize),
                    Style::default().fg(Color::White).bg(if odakli {
                        Color::Blue
                    } else {
                        Color::Black
                    }),
                )),
                kutu,
            );

            if odakli {
                let cx = deger_x + (self.cursor - kaydir) as u16;
                self.cursor_at = Some((cx.min(deger_x + deger_w.saturating_sub(1)), y));
            }
        }

        let alt_sinir = inner.y + inner.height;

        // Hata satırı.
        let hata_y = inner.y + FIELDS.len() as u16 + 1;
        if let Some(e) = &self.error {
            if hata_y < alt_sinir {
                f.render_widget(
                    Paragraph::new(Span::styled(
                        format!(" {e}"),
                        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                    )),
                    Rect { x: inner.x, y: hata_y, width: inner.width, height: 1 },
                );
            }
        }

        // Düğme satırı. Sığmayan düğme hiç çizilmez ve `Rect`i boş kalır —
        // boş `Rect` tıklama testinden de geçmez, tıklanamayan bir düğmeye
        // basılmış sayılmayız.
        let dugme_y = hata_y + 2;
        let mut x = inner.x + 1;
        let sag_sinir = inner.x + inner.width;
        let mut dugme = |f: &mut Frame, metin: &str, bg: Color| -> Rect {
            let w = metin.chars().count() as u16;
            if dugme_y >= alt_sinir || x + w > sag_sinir {
                return Rect::default();
            }
            let r = Rect { x, y: dugme_y, width: w, height: 1 };
            f.render_widget(
                Paragraph::new(Span::styled(
                    metin.to_string(),
                    Style::default().fg(Color::Black).bg(bg).add_modifier(Modifier::BOLD),
                )),
                r,
            );
            x += w + 2;
            r
        };
        self.save_area = dugme(f, " Kaydet (Enter) ", Color::Green);
        self.cancel_area = dugme(f, " Vazgeç (Esc) ", Color::Gray);
        self.reveal_area = dugme(
            f,
            if self.show_secrets {
                " Parolayı gizle (F9) "
            } else {
                " Parolayı göster (F9) "
            },
            Color::Yellow,
        );
    }
}

/// Silme onayı kutusu.
fn draw_confirm(f: &mut Frame, name: &str) {
    let satirlar = vec![
        Line::from(""),
        Line::from(vec![
            Span::raw("  '"),
            Span::styled(
                name.to_string(),
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ),
            Span::raw("' bağlantısı silinsin mi?"),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled(
                " E / Enter ",
                Style::default().fg(Color::Black).bg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" sil    "),
            Span::styled(
                " başka tuş ",
                Style::default().fg(Color::Black).bg(Color::Gray).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" vazgeç"),
        ]),
    ];
    let area = ortala(f.area(), 60, satirlar.len() as u16 + 2);
    f.render_widget(Clear, area);
    f.render_widget(
        Paragraph::new(satirlar).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
                .title(" Bağlantıyı sil "),
        ),
        area,
    );
}

/// Karakter indeksinin bayt karşılığı (UTF-8 güvenli ekleme/silme için).
fn bayt(s: &str, ci: usize) -> usize {
    s.char_indices().nth(ci).map(|(b, _)| b).unwrap_or(s.len())
}

/// "Bağlanılıyor..." ara ekranı.
pub fn draw_connecting(f: &mut Frame, name: &str) {
    let area = ortala(f.area(), 50, 3);
    let p = Paragraph::new(Line::from(vec![
        Span::raw("Bağlanılıyor: "),
        Span::styled(name, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw(" ..."),
    ]))
    .alignment(Alignment::Center)
    .block(Block::default().borders(Borders::ALL));
    f.render_widget(p, area);
}

/// Ekranı ortalayan Rect.
fn ortala(area: Rect, width: u16, height: u16) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + (area.height - h) / 2,
        width: w,
        height: h,
    }
}

/// Sunucu anahtarı sorununu gösterir. Dönüş: kullanıcı **kabul edip yeniden
/// denemek** istiyor mu?
///
/// Yalnızca `Unknown` (ilk bağlantı) kabul edilebilir. `Changed` ortadaki-adam
/// saldırısının imzasıdır — burada bir "kabul et" yolu bilerek **yoktur**;
/// kullanıcı `known_hosts`u elle düzeltmeli, yoksa tek tuşla korumayı iptal
/// etmiş oluruz.
pub async fn confirm_host_key(
    term: &mut Term,
    sc: &ServerConfig,
    issue: &crate::ssh::HostKeyIssue,
) -> Result<bool> {
    use crate::ssh::HostKeyIssue;

    let kabul_edilebilir = matches!(issue, HostKeyIssue::Unknown { .. });
    let mut events = EventStream::new();

    loop {
        term.draw(|f| draw_host_key(f, sc, issue))?;

        match events.next().await {
            Some(Ok(Event::Key(k))) if k.kind == KeyEventKind::Press => match k.code {
                KeyCode::Char('e') | KeyCode::Char('E') | KeyCode::Enter if kabul_edilebilir => {
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

fn draw_host_key(f: &mut Frame, sc: &ServerConfig, issue: &crate::ssh::HostKeyIssue) {
    use crate::ssh::HostKeyIssue;

    let kirmizi = Style::default().fg(Color::Red).add_modifier(Modifier::BOLD);
    let sari = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);
    let sonuk = Style::default().fg(Color::DarkGray);

    let (baslik, kenar, mut satirlar) = match issue {
        HostKeyIssue::Unknown { algorithm, .. } => (
            " Bilinmeyen sunucu anahtarı ",
            sari,
            vec![
                Line::from(vec![
                    Span::raw("'"),
                    Span::styled(sc.name.clone(), sari),
                    Span::raw(format!("' ({}:{}) ilk kez bağlanıyorsunuz.", sc.host, sc.port)),
                ]),
                Line::from(""),
                Line::from(format!("  Anahtar türü : {algorithm}")),
                Line::from(format!("  Parmak izi   : {}", issue.fingerprint())),
                Line::from(""),
                Line::from(Span::styled(
                    "Bu parmak izini sunucudan bağımsız bir yolla doğrulayın:",
                    sonuk,
                )),
                Line::from(Span::styled(
                    "  ssh-keyscan -p PORT HOST | ssh-keygen -lf -",
                    Style::default().fg(Color::LightCyan),
                )),
                Line::from(""),
                Line::from("Kabul ederseniz ~/.ssh/known_hosts dosyasına eklenir."),
            ],
        ),
        HostKeyIssue::Changed { line, .. } => (
            " ⚠ SUNUCU ANAHTARI DEĞİŞTİ ",
            kirmizi,
            vec![
                Line::from(Span::styled(
                    "Bu sunucunun anahtarı daha önce kaydettiğinizden FARKLI.",
                    kirmizi,
                )),
                Line::from(""),
                Line::from(format!("  {}:{}", sc.host, sc.port)),
                Line::from(format!("  Yeni parmak izi : {}", issue.fingerprint())),
                Line::from(format!("  known_hosts satırı : {line}")),
                Line::from(""),
                Line::from("Sunucu yeniden kurulduysa bu normaldir. Değilse"),
                Line::from(Span::styled(
                    "trafiğiniz araya giren biri tarafından dinleniyor olabilir.",
                    kirmizi,
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "Emin olmadan devam etmeyin. Eminseniz ilgili satırı silin:",
                    sonuk,
                )),
                Line::from(Span::styled(
                    format!("  ssh-keygen -R '[{}]:{}'", sc.host, sc.port),
                    Style::default().fg(Color::LightCyan),
                )),
            ],
        ),
        HostKeyIssue::Unreadable(e) => (
            " known_hosts okunamadı ",
            kirmizi,
            vec![
                Line::from("Sunucu anahtarı doğrulanamadı:"),
                Line::from(""),
                Line::from(Span::styled(format!("  {e}"), kirmizi)),
                Line::from(""),
                Line::from(Span::styled(
                    "~/.ssh/known_hosts dosyasının izinlerini kontrol edin.",
                    sonuk,
                )),
            ],
        ),
    };

    satirlar.push(Line::from(""));
    satirlar.push(if matches!(issue, HostKeyIssue::Unknown { .. }) {
        Line::from(vec![
            Span::styled(
                " E / Enter ",
                Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" kabul et ve bağlan    "),
            Span::styled(
                " H / Esc ",
                Style::default().fg(Color::Black).bg(Color::Gray).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" vazgeç"),
        ])
    } else {
        Line::from(vec![
            Span::styled(
                " Esc ",
                Style::default().fg(Color::Black).bg(Color::Gray).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" kapat"),
        ])
    });

    let yukseklik = satirlar.len() as u16 + 2;
    let area = ortala(f.area(), 74, yukseklik);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(kenar)
        .title(baslik);
    f.render_widget(Clear, area);
    f.render_widget(Paragraph::new(satirlar).block(block), area);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form_ile(alanlar: [&str; FIELDS.len()]) -> Form {
        let mut f = Form::new(None, &ServerConfig::default());
        for (i, v) in alanlar.iter().enumerate() {
            f.fields[i] = v.to_string();
        }
        f
    }

    fn ornek_sunucular() -> Vec<ServerConfig> {
        vec![
            ServerConfig {
                name: "Prod Sunucu".into(),
                host: "1.2.3.4".into(),
                port: 22,
                user: "okan".into(),
                password: "gizli".into(),
                ..Default::default()
            },
            ServerConfig {
                name: "Anahtarlı".into(),
                host: "sunucu.example.com".into(),
                user: "root".into(),
                key: Some("~/.ssh/id_ed25519".into()),
                key_passphrase: Some("pp".into()),
                ..Default::default()
            },
        ]
    }

    fn yonetici<'a>(servers: &'a mut Vec<ServerConfig>, path: &'a str) -> Manager<'a> {
        let mut m = Manager {
            servers,
            path,
            view: Vec::new(),
            query: String::new(),
            selected: 0,
            offset: 0,
            list_area: Rect::default(),
            buttons: Vec::new(),
            status: String::new(),
            mode: Mode::List,
            quit: false,
        };
        m.refilter();
        m
    }

    /// Çizim dar terminalde de panik etmemeli. Yerleşim elle `Rect` kurduğu
    /// (ortalanmış kutular, sabit etiket genişliği) için taşma/çıkarma
    /// hatalarının yakalanacağı tek yer burası — TUI'yi test edemiyoruz ama
    /// aritmetiğini edebiliriz.
    #[test]
    fn cizim_dar_terminalde_panik_etmez() {
        use ratatui::backend::TestBackend;

        for (w, h) in [(120u16, 40u16), (80, 24), (40, 10), (24, 6), (10, 3), (3, 2)] {
            for kip in 0..4 {
                let mut servers = ornek_sunucular();
                let mut m = yonetici(&mut servers, "config.json");
                m.mode = match kip {
                    1 => Mode::Form(Box::new(Form::new(Some(0), &ornek_sunucular()[0]))),
                    2 => Mode::Form(Box::new(Form::new(None, &ServerConfig::default()))),
                    3 => Mode::Confirm {
                        index: 0,
                        name: "Prod Sunucu".into(),
                    },
                    _ => Mode::List,
                };
                let mut t = ratatui::Terminal::new(TestBackend::new(w, h)).unwrap();
                t.draw(|f| m.draw(f))
                    .unwrap_or_else(|e| panic!("{w}x{h} kip {kip}: {e}"));
            }
        }
    }

    /// Boş listede de çizim yapılmalı (ilk çalıştırma hâli).
    #[test]
    fn bos_liste_cizilir() {
        use ratatui::backend::TestBackend;
        let mut servers = Vec::new();
        let mut m = yonetici(&mut servers, "config.json");
        let mut t = ratatui::Terminal::new(TestBackend::new(80, 24)).unwrap();
        t.draw(|f| m.draw(f)).unwrap();
        // Seçili kayıt yok; eylemler panik etmek yerine durum mesajı vermeli.
        m.act(Action::Edit);
        m.act(Action::Copy);
        m.act(Action::Delete);
        m.act(Action::MoveUp);
        assert!(matches!(m.mode, Mode::List));
    }

    /// Sıralama arama açıkken devre dışı — görünen komşu ile gerçek komşu
    /// farklı olurdu.
    #[test]
    fn siralama_arama_acikken_kapali() {
        let mut servers = ornek_sunucular();
        let mut m = yonetici(&mut servers, "config.json");
        m.query = "anahtar".into();
        m.refilter();
        m.act(Action::MoveUp);
        assert_eq!(m.servers[0].name, "Prod Sunucu", "sıra değişmemeli");
        assert!(m.status.contains("aramayı temizleyin"));
    }

    #[test]
    fn form_gecerli_kaydi_uretir() {
        let f = form_ile(["Prod", "1.2.3.4", "2222", "okan", "p4rola", "~/.ssh/id_ed25519", ""]);
        let sc = f.build().expect("geçerli form");
        assert_eq!(sc.name, "Prod");
        assert_eq!(sc.port, 2222);
        assert_eq!(sc.password, "p4rola");
        assert_eq!(sc.key.as_deref(), Some("~/.ssh/id_ed25519"));
        // Boş anahtar parolası `None` olmalı; `Some("")` config'i kirletir.
        assert!(sc.key_passphrase.is_none());
    }

    #[test]
    fn form_bos_port_22ye_duser_ad_bosken_hostu_alir() {
        let f = form_ile(["", "sunucu.local", "", "root", "", "", ""]);
        let sc = f.build().unwrap();
        assert_eq!(sc.port, 22);
        assert_eq!(sc.name, "sunucu.local", "adsız satır listede görünmesin");
        assert!(sc.password.is_empty());
    }

    #[test]
    fn form_host_ve_kullanici_zorunlu() {
        assert!(form_ile(["A", "", "22", "root", "", "", ""]).build().is_err());
        assert!(form_ile(["A", "h", "22", "", "", "", ""]).build().is_err());
        // u16'ya sığmayan port hata vermeli, panik değil.
        assert!(form_ile(["A", "h", "70000", "root", "", "", ""]).build().is_err());
        assert!(form_ile(["A", "h", "0", "root", "", "", ""]).build().is_err());
    }

    /// Parola baştaki/sondaki boşluğu korumalı; diğer alanlar trim'lenmeli.
    #[test]
    fn parola_trimlenmez_digerleri_trimlenir() {
        let f = form_ile(["  Ad  ", "  host  ", "22", "  root  ", "  gizli  ", "", ""]);
        let sc = f.build().unwrap();
        assert_eq!(sc.name, "Ad");
        assert_eq!(sc.host, "host");
        assert_eq!(sc.user, "root");
        assert_eq!(sc.password, "  gizli  ");
    }

    /// Port alanına harf yazılamaz — kaydederken hata kutusu göstermek yerine
    /// girişte engelleniyor.
    #[test]
    fn port_alani_yalniz_rakam_alir() {
        let mut f = Form::new(None, &ServerConfig::default());
        f.cur = F_PORT;
        f.fields[F_PORT].clear();
        f.cursor = 0;
        for c in "2a2b".chars() {
            f.insert(c);
        }
        assert_eq!(f.fields[F_PORT], "22");
    }

    /// Türkçe karakterler çok baytlı; imleç karakter sayar, `String` bayt.
    /// Karışırlarsa `insert`/`remove` panikler — sınır burada korunuyor.
    #[test]
    fn utf8_alanda_imlec_ve_silme_panik_etmez() {
        let mut f = Form::new(None, &ServerConfig::default());
        f.cur = F_NAME;
        for c in "İstanbul".chars() {
            f.insert(c);
        }
        assert_eq!(f.fields[F_NAME], "İstanbul");
        assert_eq!(f.cursor, 8, "imleç karakter sayar, bayt değil");

        f.cursor = 0;
        f.delete(); // baştaki çok baytlı 'İ'
        assert_eq!(f.fields[F_NAME], "stanbul");

        f.cursor = f.fields[F_NAME].chars().count();
        f.backspace();
        assert_eq!(f.fields[F_NAME], "stanbu");

        // İmleç baştayken backspace hiçbir şey yapmamalı.
        f.cursor = 0;
        f.backspace();
        assert_eq!(f.fields[F_NAME], "stanbu");
    }

    #[test]
    fn arama_ad_host_ve_kullaniciyi_tarar() {
        let s = ServerConfig {
            name: "Prod".into(),
            host: "10.0.0.5".into(),
            user: "deploy".into(),
            ..Default::default()
        };
        assert!(eslesir(&s, "prod"), "ada göre");
        assert!(eslesir(&s, "10.0"), "host'a göre");
        assert!(eslesir(&s, "deploy"), "kullanıcıya göre");
        assert!(!eslesir(&s, "test"));
    }
}


