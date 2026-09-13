//! Açılış ekranı: config.json'daki sunucuları listeler, fareyle seçtirir.

use std::io::Stdout;

use anyhow::Result;
use crossterm::event::{
    Event, EventStream, KeyCode, KeyEventKind, MouseButton, MouseEventKind,
};
use futures::StreamExt;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame, Terminal,
};

use crate::config::ServerConfig;

type Term = Terminal<CrosstermBackend<Stdout>>;

/// Sunucu listesini gösterir; seçilen sunucunun indeksini döndürür.
/// `None` = kullanıcı çıktı.
pub async fn run(term: &mut Term, servers: &[ServerConfig]) -> Result<Option<usize>> {
    let mut selected = 0usize;
    let mut list_area = Rect::default();
    let mut events = EventStream::new();

    loop {
        term.draw(|f| list_area = draw(f, servers, selected))?;

        match events.next().await {
            Some(Ok(ev)) => match ev {
                // Yalnızca Press olaylarını işle (Windows'ta çift algılama önlenir).
                Event::Key(k) if k.kind == KeyEventKind::Press => match k.code {
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(None),
                    KeyCode::Enter => return Ok(Some(selected)),
                    KeyCode::Down if selected + 1 < servers.len() => selected += 1,
                    KeyCode::Up => selected = selected.saturating_sub(1),
                    _ => {}
                },
                Event::Mouse(m) => {
                    if let Some(idx) = hit(list_area, m.column, m.row, servers.len()) {
                        match m.kind {
                            // Tek tık: seç + bağlan (fare öncelikli).
                            MouseEventKind::Down(MouseButton::Left) => {
                                return Ok(Some(idx));
                            }
                            MouseEventKind::Moved => selected = idx,
                            _ => {}
                        }
                    }
                }
                _ => {}
            },
            Some(Err(e)) => return Err(e.into()),
            None => return Ok(None),
        }
    }
}

/// "Bağlanılıyor..." ara ekranı.
pub fn draw_connecting(f: &mut Frame, name: &str) {
    let area = centered(f.area(), 50, 3);
    let p = Paragraph::new(Line::from(vec![
        Span::raw("Bağlanılıyor: "),
        Span::styled(name, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw(" ..."),
    ]))
    .alignment(Alignment::Center)
    .block(Block::default().borders(Borders::ALL));
    f.render_widget(p, area);
}

fn draw(f: &mut Frame, servers: &[ServerConfig], selected: usize) -> Rect {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(f.area());

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" SSH Sunucu Seç ");
    let inner = block.inner(root[0]);

    let items: Vec<ListItem> = servers
        .iter()
        .map(|s| {
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!(" {} ", s.name),
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{}@{}:{}", s.user, s.host, s.port),
                    Style::default().fg(Color::Gray),
                ),
            ]))
        })
        .collect();

    let list = List::new(items).block(block).highlight_style(
        Style::default()
            .bg(Color::Blue)
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    );

    let mut state = ListState::default();
    state.select(Some(selected));
    f.render_stateful_widget(list, root[0], &mut state);

    let hint = Paragraph::new(Span::styled(
        " Tıkla = bağlan · ↑/↓ + Enter · q: çıkış ",
        Style::default().fg(Color::Black).bg(Color::Cyan),
    ));
    f.render_widget(hint, root[1]);

    inner
}

/// (col,row) liste alanında mı? İse hangi satır.
fn hit(area: Rect, col: u16, row: u16, len: usize) -> Option<usize> {
    if col < area.x || col >= area.x + area.width || row < area.y || row >= area.y + area.height {
        return None;
    }
    let idx = (row - area.y) as usize;
    if idx < len {
        Some(idx)
    } else {
        None
    }
}

/// Ekranı ortalayan Rect.
fn centered(area: Rect, width: u16, height: u16) -> Rect {
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
    // Mevcut `centered` mutlak genişlik alır: 74 sütunluk kutu.
    let area = centered(f.area(), 74, yukseklik);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(kenar)
        .title(baslik);
    f.render_widget(ratatui::widgets::Clear, area);
    f.render_widget(Paragraph::new(satirlar).block(block), area);
}
