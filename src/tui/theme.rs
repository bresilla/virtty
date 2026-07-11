//! Shared visual language with `../nastty`.
//!
//! Keep virtty on the terminal's background and use the same
//! Catppuccin-Mocha-flavoured palette, rounded cards, muted tables, and
//! low-contrast selection treatment as nastty.

use ratatui::{
    style::{Color, Modifier, Style},
    text::Span,
    widgets::{Block, BorderType, Borders, Padding},
};

// ── palette ─────────────────────────────────────────────────────

pub const TEXT: Color = Color::Rgb(0xcd, 0xd6, 0xf4);
pub const SUBTEXT: Color = Color::Rgb(0xa6, 0xad, 0xc8);
pub const MUTED: Color = Color::Rgb(0x6c, 0x70, 0x86);
pub const SURFACE: Color = Color::Rgb(0x31, 0x32, 0x44);
pub const SURFACE_LO: Color = Color::Rgb(0x24, 0x25, 0x34);

// nastty's primary accent for tabs, labels, focus bars, and key chips.
pub const COLOR0: Color = Color::Indexed(0);
pub const ACCENT: Color = Color::Indexed(1);
pub const SELECTED_TAB_TEXT: Color = COLOR0;
pub const MAUVE: Color = Color::Indexed(5);
pub const GREEN: Color = Color::Rgb(0xa6, 0xe3, 0xa1);
pub const RED: Color = Color::Rgb(0xf3, 0x8b, 0xa8);
pub const YELLOW: Color = Color::Rgb(0xf9, 0xe2, 0xaf);
pub const PEACH: Color = Color::Rgb(0xfa, 0xb3, 0x87);
pub const BLUE: Color = Color::Rgb(0x89, 0xb4, 0xfa);

pub const SUCCESS: Color = GREEN;
pub const WARNING: Color = YELLOW;
pub const DANGER: Color = RED;

// ── styles ──────────────────────────────────────────────────────

pub fn text() -> Style {
    Style::default().fg(TEXT)
}

pub fn dim() -> Style {
    Style::default().fg(MUTED)
}

pub fn subtle() -> Style {
    Style::default().fg(SUBTEXT)
}

pub fn label() -> Style {
    Style::default().fg(ACCENT)
}

pub fn title() -> Style {
    Style::default().fg(MAUVE).add_modifier(Modifier::BOLD)
}

pub fn table_header() -> Style {
    Style::default()
        .fg(MAUVE)
        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
}

/// Compatibility name used by existing headings in the Incus views.
pub fn heading() -> Style {
    table_header()
}

/// Whole-card row selection, matching nastty's deliberately low-noise focus.
pub fn selected_row() -> Style {
    Style::default().bg(SURFACE)
}

pub fn muted() -> Style {
    dim()
}

pub fn status(status: &str) -> Style {
    let color = match status {
        "Running" => GREEN,
        "Stopped" => MUTED,
        "Frozen" => YELLOW,
        _ => RED,
    };
    Style::default().fg(color).add_modifier(Modifier::BOLD)
}

// ── blocks ──────────────────────────────────────────────────────

pub fn panel_bare() -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(SURFACE))
        .padding(Padding::horizontal(1))
}

pub fn panel(title: impl Into<String>) -> Block<'static> {
    panel_bare().title(Span::styled(format!(" {} ", title.into()), self::title()))
}

/// Key-hint chip used in popups, copied from nastty so modal affordances match.
pub fn chip<'a>(key: &'a str, label: &'a str) -> Vec<Span<'a>> {
    vec![
        Span::styled(
            format!(" {key} "),
            Style::default()
                .fg(SURFACE_LO)
                .bg(ACCENT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" {label}  "), dim()),
    ]
}
