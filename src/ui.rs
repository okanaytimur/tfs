//! Ratatui çizimi: iki panel + durum çubuğu + sürükleme "hayaleti".

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Gauge, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::app::{App, Panel, PanelId};

pub fn draw(f: &mut Frame, app: &mut App, server_name: &str) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // üst mod/başlık çubuğu
            Constraint::Min(3),    // paneller
            Constraint::Length(1), // durum çubuğu
        ])
        .split(f.area());

    // Üst mod çubuğu (F1/F2) — terminal moduyla ortak.
    crate::terminal::draw_top_bar(f, root[0], server_name, false, false);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(root[1]);

    draw_panel(f, cols[0], &mut app.local, app.focus == PanelId::Local, "YEREL");
    draw_panel(f, cols[1], &mut app.remote, app.focus == PanelId::Remote, "UZAK");

    // Durum çubuğu
    let status = Paragraph::new(Line::from(vec![Span::styled(
        format!(" {} ", app.status),
        Style::default().fg(Color::Black).bg(Color::Cyan),
    )]));
    f.render_widget(status, root[2]);

    // Devam eden transfer varsa ortada bir progress bar göster.
    if let Some(t) = &app.transfer {
        let area = centered_rect(f.area(), 60, 3);
        let ratio = if t.total > 0 {
            (t.done as f64 / t.total as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let pct = (ratio * 100.0) as u16;
        let label = if t.total > 0 {
            format!(
                "{}  {} / {}  ({pct}%)",
                t.name,
                human_bytes(t.done),
                human_bytes(t.total),
            )
        } else {
            format!("{}  {}", t.name, human_bytes(t.done))
        };
        let gauge = Gauge::default()
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
                    .title(" Transfer — q/Esc: iptal "),
            )
            .gauge_style(Style::default().fg(Color::Green).bg(Color::Black))
            .ratio(ratio)
            .label(label);
        f.render_widget(Clear, area);
        f.render_widget(gauge, area);
    }

    // Sürükleme hayaleti (fare pozisyonunda yüzen etiket)
    if let Some(d) = &app.drag {
        if d.active {
            let label = format!(" ⇢ {} ", d.entry.name);
            let w = (label.chars().count() as u16).min(f.area().width.saturating_sub(1));
            let x = d.col.min(f.area().width.saturating_sub(w));
            let y = d.row.min(f.area().height.saturating_sub(1));
            let area = Rect {
                x,
                y,
                width: w,
                height: 1,
            };
            f.render_widget(Clear, area);
            f.render_widget(
                Paragraph::new(label).style(
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                area,
            );
        }
    }
}

fn draw_panel(f: &mut Frame, area: Rect, panel: &mut Panel, focused: bool, tag: &str) {
    let border_style = if focused {
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let title = format!(" {tag}: {} ", panel.cwd);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(title);

    let inner = block.inner(area);
    panel.list_area = inner; // fare isabet testi için kaydet

    let items: Vec<ListItem> = panel
        .entries
        .iter()
        .map(|e| {
            let (icon, style) = if e.is_dir {
                ("📁 ", Style::default().fg(Color::LightBlue))
            } else {
                ("📄 ", Style::default().fg(Color::White))
            };
            ListItem::new(Line::from(vec![
                Span::raw(icon),
                Span::styled(e.name.clone(), style),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_style(
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    let mut state = ListState::default();
    state.select(Some(panel.selected));
    *state.offset_mut() = panel.offset;

    f.render_stateful_widget(list, area, &mut state);
}

/// Ekranın ortasında, verilen genişlik (yüzde) ve yükseklikte (satır) bir dikdörtgen.
fn centered_rect(area: Rect, percent_x: u16, height: u16) -> Rect {
    let width = area.width * percent_x / 100;
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect {
        x,
        y,
        width,
        height: height.min(area.height),
    }
}

/// Baytları insan-okur biçime çevirir (B, KiB, MiB, GiB).
fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{n} B")
    } else {
        format!("{v:.1} {}", UNITS[u])
    }
}
