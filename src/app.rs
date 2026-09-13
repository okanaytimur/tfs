//! Uygulama durumu + fare/drag-drop mantığı (fare öncelikli UX).

use std::path::PathBuf;

use anyhow::Result;
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
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
///
/// `entries` dizinin **tamamıdır**; `view` ise arama sorgusuna uyan girdilerin
/// `entries` içindeki indeksleridir. Seçim, kaydırma ve fare isabet testi
/// daima `view` üzerinden çalışır — böylece filtre açıkken de gizli bir girdi
/// yanlışlıkla seçilemez.
pub struct Panel {
    /// Yerel için tam yol, uzak için `/...` string yolu.
    pub cwd: String,
    pub entries: Vec<Entry>,
    /// `entries` içindeki görünür indeksler (sorguya uyanlar).
    pub view: Vec<usize>,
    /// Arama sorgusu; boşsa her şey görünür.
    pub query: String,
    /// `view` içindeki seçili konum.
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
            view: Vec::new(),
            query: String::new(),
            selected: 0,
            offset: 0,
            list_area: Rect::default(),
        }
    }

    /// Görünür girdiler (çizim için).
    pub fn visible(&self) -> impl Iterator<Item = &Entry> {
        self.view.iter().filter_map(|&i| self.entries.get(i))
    }

    /// O an seçili girdi.
    pub fn current(&self) -> Option<&Entry> {
        self.entries.get(*self.view.get(self.selected)?)
    }

    /// `view`i sorguya göre yeniden kurar. Seçili girdi hâlâ görünüyorsa
    /// **seçim onun üzerinde kalır** — harf sildikçe imlecin zıplamaması için.
    fn refilter(&mut self) {
        let onceki = self.view.get(self.selected).copied();
        let q = fold(&self.query);
        self.view = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| q.is_empty() || fold(&e.name).contains(&q))
            .map(|(i, _)| i)
            .collect();
        self.selected = onceki
            .and_then(|i| self.view.iter().position(|&v| v == i))
            .unwrap_or(0)
            .min(self.view.len().saturating_sub(1));
        self.offset = 0;
        self.clamp_scroll();
    }

    /// Sorguyu temizler (seçimi elden geldiğince korur).
    fn clear_query(&mut self) {
        self.query.clear();
        self.refilter();
    }

    /// (col,row) bu panelin liste alanında mı? Ise `view` içinde hangi konumda?
    fn hit(&self, col: u16, row: u16) -> Option<usize> {
        let a = self.list_area;
        if col < a.x || col >= a.x + a.width || row < a.y || row >= a.y + a.height {
            return None;
        }
        let rel = (row - a.y) as usize;
        let idx = self.offset + rel;
        if idx < self.view.len() {
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

/// Arama karşılaştırması için normalleştirme: küçük harfe indirger.
///
/// Türkçe notu: `to_lowercase` Unicode kurallarını uygular, yani `I` → `i`
/// (Türkçedeki `ı` değil). Dosya adları çoğunlukla ASCII olduğu için bu
/// pratikte doğru davranış; "INDEX" yazınca `index.html` bulunur.
fn fold(s: &str) -> String {
    s.to_lowercase()
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

/// F4 ile doğan düzenleme isteği; ana döngü bunu alıp editörü açar
/// (bkz. `main::run_edit`).
#[derive(Clone)]
pub struct EditRequest {
    /// Dosyanın hangi panelden geldiği. `Remote` ise indir → düzenle → yükle.
    pub panel: PanelId,
    pub name: String,
    /// Yerel panelde: düzenlenecek dosya. Uzak panelde: kullanılmaz.
    pub local_path: PathBuf,
    /// Uzak panelde: indirilecek/geri yüklenecek yol. Yerelde boş.
    pub remote_path: String,
}

/// Devam eden transferin UI'da gösterilen ilerlemesi.
pub struct TransferState {
    /// Transferin kökü (sürüklenen/seçilen dosya ya da klasör adı).
    pub name: String,
    pub done: u64,
    pub total: u64,
    /// Şu an ne yapılıyor: "taranıyor…" ya da aktarılan dosyanın köke göre yolu.
    pub label: Option<String>,
    /// (tamamlanan, toplam) dosya sayısı — klasör transferinde dolu.
    pub files: Option<(u32, u32)>,
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
    /// Bekleyen düzenleme isteği; ana döngü editörü açar.
    pub pending_edit: Option<EditRequest>,
}

impl App {
    pub fn new() -> Self {
        Self {
            local: Panel::new(),
            remote: Panel::new(),
            focus: PanelId::Local,
            drag: None,
            status: "Yazmaya başla = ara · F5: transfer · F4: düzenle · Esc: temizle/çık".into(),
            should_quit: false,
            pending_transfer: None,
            transfer: None,
            pending_edit: None,
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
        // Yeni dizin, yeni bağlam: eski sorgu burada anlamsız.
        self.local.query.clear();
        self.local.refilter();
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
        self.remote.query.clear();
        self.remote.refilter();
        Ok(())
    }

    // --- Navigasyon ---

    /// `idx` = `view` içindeki konum (fare tıklaması ya da seçim).
    async fn enter(&mut self, panel: PanelId, idx: usize, ssh: &Ssh) -> Result<()> {
        let p = self.panel_ref(panel);
        let entry = match p.view.get(idx).and_then(|&i| p.entries.get(i)).cloned() {
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

    /// Bir üst dizine çıkar. (Eskiden `enter(idx = 0)` ile yapılıyordu; filtre
    /// açıkken 0. görünür girdi `..` olmayabileceği için artık doğrudan.)
    async fn go_parent(&mut self, panel: PanelId, ssh: &Ssh) -> Result<()> {
        match panel {
            PanelId::Local => {
                let cur = PathBuf::from(&self.local.cwd);
                let next = cur.parent().map(|p| p.to_path_buf()).unwrap_or(cur);
                if let Err(e) = self.load_local(next) {
                    self.status = format!("Yerel dizin hatası: {e}");
                }
            }
            PanelId::Remote => {
                let next = ssh::remote_parent(&self.remote.cwd);
                if let Err(e) = self.load_remote(ssh, next).await {
                    self.status = format!("Uzak dizin hatası: {e}");
                }
            }
        }
        Ok(())
    }

    // --- Transfer ---

    /// Bir transfer *isteği* kuyruklar. Gerçek I/O ana döngüde ayrı bir task'te
    /// yapılır (bkz. `main::run_transfer`), böylece UI bloklanmaz ve progress
    /// bar canlı kalır.
    ///
    /// Hedef yol daima **karşı panelin o anki dizini** + aynı dosya adıdır.
    fn queue_transfer(&mut self, source: PanelId, entry: &Entry, target: PanelId) {
        if entry.is_dir && entry.name == ".." {
            self.status = "Üst dizin (`..`) aktarılamaz.".into();
            return;
        }
        if source == target {
            self.status = "Kaynak ve hedef aynı panel — işlem yok.".into();
            return;
        }
        let local_path = PathBuf::from(&self.local.cwd).join(&entry.name);
        let remote_path = ssh::remote_join(&self.remote.cwd, &entry.name);
        self.pending_transfer = Some(TransferRequest {
            source,
            target,
            name: entry.name.clone(),
            local_path,
            remote_path,
        });
    }

    /// Sürükle-bırak sonucu transfer.
    fn request_transfer(&mut self, drag: &Drag, target: PanelId) {
        self.queue_transfer(drag.source, &drag.entry, target);
    }

    /// `t` (ya da F5): odaklı paneldeki seçili dosyayı **karşı panele** aktarır.
    /// Fare kullanmadan transfer — sürükle-bırakla aynı işi yapar.
    fn request_transfer_selected(&mut self) {
        let source = self.focus;
        let target = match source {
            PanelId::Local => PanelId::Remote,
            PanelId::Remote => PanelId::Local,
        };
        let entry = match self.panel_ref(source).current() {
            Some(e) => e.clone(),
            None => return,
        };
        self.queue_transfer(source, &entry, target);
    }

    /// Panelde adı verilen girdiyi seçer (varsa). Bir yenilemeden sonra
    /// kullanıcının imlecini kaybetmemek için: `load_local`/`load_remote`
    /// seçimi sıfırlar.
    pub fn select_by_name(&mut self, panel: PanelId, name: &str) {
        let p = self.panel_mut(panel);
        let Some(entry_idx) = p.entries.iter().position(|e| e.name == name) else {
            return;
        };
        // Girdi filtrenin dışında kalıyorsa seçilemez; sorguyu temizle.
        if !p.view.contains(&entry_idx) {
            p.clear_query();
        }
        if let Some(pos) = p.view.iter().position(|&v| v == entry_idx) {
            p.selected = pos;
            p.clamp_scroll();
        }
    }

    /// Panelde o an seçili olan girdinin adı (yenileme öncesi saklamak için).
    pub fn selected_name(&self, panel: PanelId) -> Option<String> {
        self.panel_ref(panel).current().map(|e| e.name.clone())
    }

    // --- Düzenleme (F4) ---

    /// Odaklı panelde seçili dosya için bir düzenleme *isteği* oluşturur.
    /// Gerçek iş (indirme, editörü açma, geri yükleme) ana döngüdedir
    /// (`main::run_edit`) — editör TUI'yi askıya aldığı için burada yapılamaz.
    fn request_edit(&mut self) {
        let panel = self.focus;
        let entry = match self.panel_ref(panel).current() {
            Some(e) => e.clone(),
            None => return,
        };
        if entry.is_dir {
            self.status = "Klasör düzenlenemez — bir dosya seçin.".into();
            return;
        }
        let (local_path, remote_path) = match panel {
            PanelId::Local => (
                PathBuf::from(&self.local.cwd).join(&entry.name),
                String::new(),
            ),
            PanelId::Remote => (
                PathBuf::new(),
                ssh::remote_join(&self.remote.cwd, &entry.name),
            ),
        };
        self.pending_edit = Some(EditRequest {
            panel,
            name: entry.name,
            local_path,
            remote_path,
        });
    }

    // --- Olay işleme ---

    pub async fn handle_event(&mut self, ev: Event, ssh: &Ssh) -> Result<()> {
        match ev {
            // Windows'ta crossterm hem Press hem Release üretir; çift algılamayı
            // önlemek için yalnızca Press olaylarını işle.
            Event::Key(k) if k.kind == KeyEventKind::Press => {
                self.handle_key(k, ssh).await?
            }
            Event::Key(_) => {}
            Event::Mouse(m) => self.handle_mouse(m, ssh).await?,
            _ => {}
        }
        Ok(())
    }

    /// Dosya modunda tuşlar.
    ///
    /// **Yazılabilir her karakter arama sorgusuna gider** — bu yüzden eski tek
    /// harfli kısayollar (`q`/`t`/`e`) yoktur; yerlerine F-tuşları (Norton/MC
    /// geleneği) ve `Ctrl+Q` geçti.
    async fn handle_key(&mut self, k: KeyEvent, ssh: &Ssh) -> Result<()> {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let alt = k.modifiers.contains(KeyModifiers::ALT);

        match k.code {
            // --- Çıkış ---
            KeyCode::Char('q') | KeyCode::Char('Q') if ctrl => self.should_quit = true,
            KeyCode::F(10) => self.should_quit = true,
            // Esc önce aramayı temizler; arama yokken çıkar.
            KeyCode::Esc => {
                let p = self.panel_mut(self.focus);
                if p.query.is_empty() {
                    self.should_quit = true;
                } else {
                    p.clear_query();
                }
            }

            // --- Eylemler ---
            KeyCode::F(4) => self.request_edit(),
            KeyCode::F(5) => self.request_transfer_selected(),
            KeyCode::Tab => {
                self.focus = match self.focus {
                    PanelId::Local => PanelId::Remote,
                    PanelId::Remote => PanelId::Local,
                }
            }

            // --- Arama düzenleme ---
            // Sorgu varken Backspace harf siler, yokken üst dizine çıkar.
            KeyCode::Backspace => {
                let p = self.panel_mut(self.focus);
                if p.query.is_empty() {
                    self.go_parent(self.focus, ssh).await?;
                } else {
                    p.query.pop();
                    p.refilter();
                }
            }
            // Yazılabilir karakter → sorguya ekle. Ctrl/Alt'lı olanlar kısayol
            // olabileceği için dışarıda bırakılır.
            KeyCode::Char(c) if !ctrl && !alt => {
                let p = self.panel_mut(self.focus);
                p.query.push(c);
                p.refilter();
            }

            // --- Gezinme ---
            KeyCode::Enter => {
                let panel = self.focus;
                let idx = self.panel_ref(panel).selected;
                self.enter(panel, idx, ssh).await?;
            }
            KeyCode::Down => {
                let p = self.panel_mut(self.focus);
                if p.selected + 1 < p.view.len() {
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
            KeyCode::Home => {
                let p = self.panel_mut(self.focus);
                p.selected = 0;
                p.clamp_scroll();
            }
            KeyCode::End => {
                let p = self.panel_mut(self.focus);
                p.selected = p.view.len().saturating_sub(1);
                p.clamp_scroll();
            }
            KeyCode::PageDown | KeyCode::PageUp => {
                let p = self.panel_mut(self.focus);
                let page = (p.list_area.height as usize).max(1);
                p.selected = if k.code == KeyCode::PageDown {
                    (p.selected + page).min(p.view.len().saturating_sub(1))
                } else {
                    p.selected.saturating_sub(page)
                };
                p.clamp_scroll();
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
                        let entry = match p.current() {
                            Some(e) => e.clone(),
                            None => break,
                        };
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
                    let max = p.view.len().saturating_sub(1);
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

/// Arama/filtre mantığı testleri. Seçim `view` üzerinden yürüdüğü için burada
/// bir hata "gizli bir dosyayı yanlışlıkla transfer etmek" demek olurdu.
#[cfg(test)]
mod tests {
    use super::*;

    fn panel(isimler: &[(&str, bool)]) -> Panel {
        let mut p = Panel::new();
        p.entries = isimler
            .iter()
            .map(|(n, d)| Entry {
                name: (*n).to_string(),
                is_dir: *d,
            })
            .collect();
        p.refilter();
        p
    }

    fn ornek() -> Panel {
        panel(&[
            ("..", true),
            ("assets", true),
            ("index.html", false),
            ("index.php", false),
            ("INDEX.md", false),
            ("robots.txt", false),
        ])
    }

    fn gorunen(p: &Panel) -> Vec<String> {
        p.visible().map(|e| e.name.clone()).collect()
    }

    #[test]
    fn bos_sorguda_hepsi_gorunur() {
        let p = ornek();
        assert_eq!(p.view.len(), 6);
        assert_eq!(p.current().unwrap().name, "..");
    }

    #[test]
    fn sorgu_filtreler_ve_buyuk_kucuk_harf_duyarsiz() {
        let mut p = ornek();
        p.query = "index".into();
        p.refilter();
        assert_eq!(gorunen(&p), ["index.html", "index.php", "INDEX.md"]);

        p.query = "INDEX".into();
        p.refilter();
        assert_eq!(gorunen(&p), ["index.html", "index.php", "INDEX.md"]);
    }

    /// `..` de filtreye tabidir; sorgu varken listeyi kirletmez.
    #[test]
    fn ust_dizin_girdisi_de_filtrelenir() {
        let mut p = ornek();
        p.query = "idx".into();
        p.refilter();
        assert!(gorunen(&p).is_empty());
    }

    /// Harf silerken imleç seçili dosyanın üstünde kalmalı — yoksa yazdıkça
    /// seçim zıplar ve yanlış dosya aktarılır.
    #[test]
    fn harf_silince_secim_korunur() {
        let mut p = ornek();
        p.query = "index".into();
        p.refilter();
        p.selected = 2; // INDEX.md
        assert_eq!(p.current().unwrap().name, "INDEX.md");

        p.query.pop(); // "inde"
        p.refilter();
        assert_eq!(p.current().unwrap().name, "INDEX.md", "seçim aynı dosyada kalmalı");

        p.query.clear();
        p.refilter();
        assert_eq!(p.current().unwrap().name, "INDEX.md");
    }

    /// Seçili girdi filtreden düşerse seçim listenin başına iner ve **geçerli**
    /// bir girdiyi gösterir (aralık dışı kalmaz).
    #[test]
    fn secim_filtreden_dusunce_gecerli_kalir() {
        let mut p = ornek();
        p.selected = 5; // robots.txt
        p.query = "index".into();
        p.refilter();
        assert_eq!(p.selected, 0);
        assert_eq!(p.current().unwrap().name, "index.html");
    }

    #[test]
    fn eslesme_yoksa_secili_girdi_de_yok() {
        let mut p = ornek();
        p.query = "zzz".into();
        p.refilter();
        assert!(p.view.is_empty());
        assert!(p.current().is_none(), "gizli bir girdi seçili görünmemeli");
    }

    /// Fare isabet testi `view` üzerinden olmalı: filtre açıkken 2. satıra
    /// tıklamak, tam listedeki 2. girdiyi değil görünen 2. girdiyi seçmeli.
    #[test]
    fn fare_isabeti_gorunen_listeye_gore() {
        let mut p = ornek();
        p.list_area = Rect { x: 0, y: 0, width: 20, height: 10 };
        p.query = "index".into();
        p.refilter();
        assert_eq!(p.hit(1, 1), Some(1));
        p.selected = p.hit(1, 1).unwrap();
        assert_eq!(p.current().unwrap().name, "index.php");
        // Görünen liste 3 satır; 4. satır boş.
        assert_eq!(p.hit(1, 3), None);
    }

    /// Filtre dışındaki bir girdiyi ada göre seçmek (transfer sonrası imleci
    /// geri koymak gibi) sorguyu temizleyip girdiyi bulmalı.
    #[test]
    fn ada_gore_secim_filtreyi_gerekirse_temizler() {
        let mut app = App::new();
        app.local = ornek();
        app.local.query = "index".into();
        app.local.refilter();

        app.select_by_name(PanelId::Local, "robots.txt");
        assert!(app.local.query.is_empty(), "sorgu temizlenmeliydi");
        assert_eq!(app.local.current().unwrap().name, "robots.txt");
    }
}
