//! Uygulama durumu + fare/drag-drop mantığı (fare öncelikli UX).

use std::path::PathBuf;

use anyhow::Result;
use crossterm::event::{Event, KeyCode, KeyEventKind, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use crate::ssh::{self, Ssh};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PanelId {
    Local,
    Remote,
}

/// Bir panel girdisi (yerel ya da uzak, aynı şekilde gösterilir).
#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub is_dir: bool,
}

/// Tek bir dosya paneli.
pub struct Panel {
    /// Yerel için tam yol, uzak için `/...` string yolu.
    pub cwd: String,
    pub entries: Vec<Entry>,
    pub selected: usize,
    pub offset: usize,
    /// Son çizilen liste iç alanı (fare isabet testi için).
    pub list_area: Rect,
}

impl Panel {
    fn new() -> Self {
        Self {
            cwd: String::new(),
            entries: Vec::new(),
            selected: 0,
            offset: 0,
            list_area: Rect::default(),
        }
    }

    /// (col,row) bu panelin liste alanında mı? Ise hangi girdi indeksinde?
    fn hit(&self, col: u16, row: u16) -> Option<usize> {
        let a = self.list_area;
        if col < a.x || col >= a.x + a.width || row < a.y || row >= a.y + a.height {
            return None;
        }
        let rel = (row - a.y) as usize;
        let idx = self.offset + rel;
        if idx < self.entries.len() {
            Some(idx)
        } else {
            None
        }
    }

    fn contains(&self, col: u16, row: u16) -> bool {
        let a = self.list_area;
        col >= a.x && col < a.x + a.width && row >= a.y && row < a.y + a.height
    }

    fn clamp_scroll(&mut self) {
        let h = self.list_area.height as usize;
        if h == 0 {
            return;
        }
        if self.selected < self.offset {
            self.offset = self.selected;
        } else if self.selected >= self.offset + h {
            self.offset = self.selected + 1 - h;
        }
    }
}

/// Sürükleme durumu. `active`, farenin gerçekten hareket ettiğini (tıklama değil) belirtir.
pub struct Drag {
    pub source: PanelId,
    pub entry: Entry,
    pub col: u16,
    pub row: u16,
    pub active: bool,
}

/// Sürükle-bırak sonucu doğan, ana döngüde (ayrı task'te) çalıştırılacak transfer isteği.
#[derive(Clone)]
pub struct TransferRequest {
    pub source: PanelId,
    pub target: PanelId,
    pub name: String,
    pub local_path: PathBuf,
    pub remote_path: String,
}

/// Devam eden transferin UI'da gösterilen ilerlemesi.
pub struct TransferState {
    pub name: String,
    pub done: u64,
    pub total: u64,
}

pub struct App {
    pub local: Panel,
    pub remote: Panel,
    pub focus: PanelId,
    pub drag: Option<Drag>,
    pub status: String,
    pub should_quit: bool,
    /// Bekleyen transfer isteği; ana döngü bunu alıp ayrı task'te çalıştırır.
    pub pending_transfer: Option<TransferRequest>,
    /// Devam eden transferin ilerlemesi (progress bar için).
    pub transfer: Option<TransferState>,
}

impl App {
    pub fn new() -> Self {
        Self {
            local: Panel::new(),
            remote: Panel::new(),
            focus: PanelId::Local,
            drag: None,
            status: "Fareyle sürükle-bırak ile transfer. q: çıkış".into(),
            should_quit: false,
            pending_transfer: None,
            transfer: None,
        }
    }

    fn panel_mut(&mut self, id: PanelId) -> &mut Panel {
        match id {
            PanelId::Local => &mut self.local,
            PanelId::Remote => &mut self.remote,
        }
    }

    // --- Listeleme ---

    pub fn load_local(&mut self, dir: PathBuf) -> Result<()> {
        let mut entries = vec![Entry {
            name: "..".into(),
            is_dir: true,
        }];
        for de in std::fs::read_dir(&dir)? {
            let de = de?;
            let name = de.file_name().to_string_lossy().to_string();
            let is_dir = de.file_type().map(|t| t.is_dir()).unwrap_or(false);
            entries.push(Entry { name, is_dir });
        }
        entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name)));
        self.local.cwd = dir.to_string_lossy().to_string();
        self.local.entries = entries;
        self.local.selected = 0;
        self.local.offset = 0;
        Ok(())
    }

    pub async fn load_remote(&mut self, ssh: &Ssh, dir: String) -> Result<()> {
        let list = ssh.list_dir(&dir).await?;
        self.remote.cwd = dir;
        self.remote.entries = list
            .into_iter()
            .map(|e| Entry {
                name: e.name,
                is_dir: e.is_dir,
            })
            .collect();
        self.remote.selected = 0;
        self.remote.offset = 0;
        Ok(())
    }

    // --- Navigasyon ---

    async fn enter(&mut self, panel: PanelId, idx: usize, ssh: &Ssh) -> Result<()> {
        let entry = match self.panel_mut(panel).entries.get(idx).cloned() {
            Some(e) => e,
            None => return Ok(()),
        };
        if !entry.is_dir {
            return Ok(());
        }
        match panel {
            PanelId::Local => {
                let cur = PathBuf::from(&self.local.cwd);
                let next = if entry.name == ".." {
                    cur.parent().map(|p| p.to_path_buf()).unwrap_or(cur)
                } else {
                    cur.join(&entry.name)
                };
                if let Err(e) = self.load_local(next) {
                    self.status = format!("Yerel dizin hatası: {e}");
                }
            }
            PanelId::Remote => {
                let next = if entry.name == ".." {
                    ssh::remote_parent(&self.remote.cwd)
                } else {
                    ssh::remote_join(&self.remote.cwd, &entry.name)
                };
                if let Err(e) = self.load_remote(ssh, next).await {
                    self.status = format!("Uzak dizin hatası: {e}");
                }
            }
        }
        Ok(())
    }

    // --- Transfer (drag-drop sonucu) ---

    /// Sürükle-bırak sonucu bir transfer *isteği* oluşturur. Gerçek I/O ana
    /// döngüde ayrı bir task'te yapılır (bkz. `main::run_transfer`), böylece UI
    /// bloklanmaz ve progress bar canlı kalır.
    fn request_transfer(&mut self, drag: &Drag, target: PanelId) {
        if drag.entry.is_dir {
            self.status = "Klasör transferi henüz desteklenmiyor (skeleton).".into();
            return;
        }
        match (drag.source, target) {
            (PanelId::Local, PanelId::Remote) => {
                let local_path = PathBuf::from(&self.local.cwd).join(&drag.entry.name);
                let remote_path = ssh::remote_join(&self.remote.cwd, &drag.entry.name);
                self.pending_transfer = Some(TransferRequest {
                    source: PanelId::Local,
                    target: PanelId::Remote,
                    name: drag.entry.name.clone(),
                    local_path,
                    remote_path,
                });
            }
            (PanelId::Remote, PanelId::Local) => {
                let remote_path = ssh::remote_join(&self.remote.cwd, &drag.entry.name);
                let local_path = PathBuf::from(&self.local.cwd).join(&drag.entry.name);
                self.pending_transfer = Some(TransferRequest {
                    source: PanelId::Remote,
                    target: PanelId::Local,
                    name: drag.entry.name.clone(),
                    local_path,
                    remote_path,
                });
            }
            _ => {
                self.status = "Aynı panele bırakıldı — işlem yok.".into();
            }
        }
    }

    // --- Olay işleme ---

    pub async fn handle_event(&mut self, ev: Event, ssh: &Ssh) -> Result<()> {
        match ev {
            // Windows'ta crossterm hem Press hem Release üretir; çift algılamayı
            // önlemek için yalnızca Press olaylarını işle.
            Event::Key(k) if k.kind == KeyEventKind::Press => {
                self.handle_key(k.code, ssh).await?
            }
            Event::Key(_) => {}
            Event::Mouse(m) => self.handle_mouse(m, ssh).await?,
            _ => {}
        }
        Ok(())
    }

    async fn handle_key(&mut self, code: KeyCode, ssh: &Ssh) -> Result<()> {
        match code {
            KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
            KeyCode::Tab => {
                self.focus = match self.focus {
                    PanelId::Local => PanelId::Remote,
                    PanelId::Remote => PanelId::Local,
                }
            }
            KeyCode::Backspace => {
                // Odaklı panelde bir üst dizin.
                let (panel, idx) = (self.focus, 0usize); // 0 = ".."
                self.enter(panel, idx, ssh).await?;
            }
            KeyCode::Enter => {
                let panel = self.focus;
                let idx = self.panel_mut(panel).selected;
                self.enter(panel, idx, ssh).await?;
            }
            KeyCode::Down => {
                let p = self.panel_mut(self.focus);
                if p.selected + 1 < p.entries.len() {
                    p.selected += 1;
                    p.clamp_scroll();
                }
            }
            KeyCode::Up => {
                let p = self.panel_mut(self.focus);
                if p.selected > 0 {
                    p.selected -= 1;
                    p.clamp_scroll();
                }
            }
            _ => {}
        }
        Ok(())
    }

    async fn handle_mouse(&mut self, m: MouseEvent, ssh: &Ssh) -> Result<()> {
        let (col, row) = (m.column, m.row);
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                // Hangi panel? Girdi seç ve olası sürükleyi kaydet.
                for id in [PanelId::Local, PanelId::Remote] {
                    if let Some(idx) = self.panel_ref(id).hit(col, row) {
                        self.focus = id;
                        let p = self.panel_mut(id);
                        p.selected = idx;
                        let entry = p.entries[idx].clone();
                        self.drag = Some(Drag {
                            source: id,
                            entry,
                            col,
                            row,
                            active: false,
                        });
                        break;
                    }
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if let Some(d) = self.drag.as_mut() {
                    d.active = true;
                    d.col = col;
                    d.row = row;
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                if let Some(drag) = self.drag.take() {
                    if drag.active {
                        // Bırakılan panel?
                        let target = if self.local.contains(col, row) {
                            Some(PanelId::Local)
                        } else if self.remote.contains(col, row) {
                            Some(PanelId::Remote)
                        } else {
                            None
                        };
                        if let Some(target) = target {
                            self.request_transfer(&drag, target);
                        }
                    } else {
                        // Sürükleme yok = tıklama: klasörse içine gir.
                        if let Some(idx) = self.panel_ref(drag.source).hit(col, row) {
                            self.enter(drag.source, idx, ssh).await?;
                        }
                    }
                }
            }
            MouseEventKind::ScrollDown => {
                let id = self.panel_at(col, row);
                if let Some(id) = id {
                    let p = self.panel_mut(id);
                    let max = p.entries.len().saturating_sub(1);
                    p.selected = (p.selected + 1).min(max);
                    p.clamp_scroll();
                }
            }
            MouseEventKind::ScrollUp => {
                if let Some(id) = self.panel_at(col, row) {
                    let p = self.panel_mut(id);
                    p.selected = p.selected.saturating_sub(1);
                    p.clamp_scroll();
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn panel_ref(&self, id: PanelId) -> &Panel {
        match id {
            PanelId::Local => &self.local,
            PanelId::Remote => &self.remote,
        }
    }

    fn panel_at(&self, col: u16, row: u16) -> Option<PanelId> {
        if self.local.contains(col, row) {
            Some(PanelId::Local)
        } else if self.remote.contains(col, row) {
            Some(PanelId::Remote)
        } else {
            None
        }
    }
}
