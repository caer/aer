use std::time::Duration;

use arboard::Clipboard;
use cate::tool::color::Neutrals;
use ratatui::{
    DefaultTerminal,
    buffer::Buffer,
    crossterm::event::{self, Event, KeyCode, KeyEventKind},
    layout::{Constraint, Flex, Layout, Rect},
    style::{Color, Stylize},
    text::Text,
    widgets::{Block, BorderType, Padding, Paragraph, Widget},
};

/// The default neutral color loaded on application start.
const DEFAULT_NEUTRAL_COLOR: &str = "E9E2D0";

/// The amount of Chroma added or removed from
/// the neutral color during each user input.
const NEUTRAL_CHROMA_STEP: f32 = 0.005;

/// The maximum Chroma value assigned to the neutral color.
const NEUTRAL_MAX_CHROMA: f32 = 1.0;

/// The number of degrees to shift hue by between
/// each neutral-derived accent color.
const ACCENT_HUE_STEP: f32 = 45.0;

/// The minimum Chroma value assigned to each accent color.
const ACCENT_MIN_CHROMA: f32 = 0.05;

fn main() -> std::io::Result<()> {
    let terminal = ratatui::init();
    let app_result = App::default().run(terminal);
    ratatui::restore();
    app_result
}

#[derive(Debug, Default)]
struct App {
    colors_widget: ColorsWidget,
}

/// A widget that displays the full range of RGB colors that can be displayed in the terminal.
///
/// This widget is animated and will change colors over time.
#[derive(Debug, Default)]
struct ColorsWidget {
    /// The base neutral color from which all neutral tones are derived.
    base_neutral_color: cate::tool::color::Color,

    /// The chromaticity of the base accent color.
    base_accent_chromaticity: f32,

    /// The hue offset of the base accent color relative
    /// to the base neutral color.
    base_accent_hue_offset: f32,

    /// Iff true, colors will be fitted into a CMYK gamut.
    cmyk_gamut_fitting: bool,

    /// The currently selected color block in the UI.
    active_color_block_index: usize,
}

impl App {
    /// Run the app.
    ///
    /// This is the main event loop for the app.
    pub fn run(mut self, mut terminal: DefaultTerminal) -> std::io::Result<()> {
        self.colors_widget.base_neutral_color =
            cate::tool::color::Color::try_from_hex(DEFAULT_NEUTRAL_COLOR.into()).unwrap();
        self.colors_widget.base_accent_chromaticity = self
            .colors_widget
            .base_neutral_color
            .c
            .max(ACCENT_MIN_CHROMA);
        self.colors_widget.base_accent_hue_offset = ACCENT_HUE_STEP;
        self.colors_widget.active_color_block_index = 0;

        loop {
            terminal.draw(|frame| frame.render_widget(&mut self, frame.area()))?;

            if !self.handle_events()? {
                break;
            }
        }

        Ok(())
    }

    /// Handle any events that have occurred since the last time the app was rendered.
    ///
    /// Returns true if the app should continue running.
    fn handle_events(&mut self) -> std::io::Result<bool> {
        // Ensure that the app only blocks for a period that allows the app to render at
        // approximately 60 FPS (this doesn't account for the time to render the frame, and will
        // also update the app immediately any time an event occurs)
        let timeout = Duration::from_secs_f32(1.0 / 60.0);
        if event::poll(timeout)?
            && let Event::Key(key) = event::read()?
        {
            // Exit the application.
            if key.kind == KeyEventKind::Press && key.code == KeyCode::Char('q') {
                return Ok(false);
            }

            // Toggle CMYK color gamut fitting.
            if key.kind == KeyEventKind::Press && key.code == KeyCode::Char('g') {
                self.colors_widget.cmyk_gamut_fitting = !self.colors_widget.cmyk_gamut_fitting;
                return Ok(true);
            }

            // Copy the current neutral colors to the keyboard as SCSS RGBA colors.
            if key.kind == KeyEventKind::Press && key.code == KeyCode::Char('w') {
                let mut neutrals =
                    Neutrals::from_color_hue_adjusted(&self.colors_widget.base_neutral_color);

                let base_color_str = format!(
                    "{} (sRGB HEX) | oklch({:.2} {:.3} {:.2})",
                    &self.colors_widget.base_neutral_color,
                    self.colors_widget.base_neutral_color.l,
                    self.colors_widget.base_neutral_color.c,
                    self.colors_widget.base_neutral_color.h,
                );

                let gamut_str = if self.colors_widget.cmyk_gamut_fitting {
                    neutrals = neutrals.to_cmyk_adjusted();
                    "(in Coated GRACoL 2006 CMYK Gamut)"
                } else {
                    "(in sRGB Gamut)"
                };

                let colors = format!(
                    r#"// {base_color_str}
$c-lightest: rgba({}, 1); // L={:.2} {gamut_str}
$c-lighter:  rgba({}, 1); // L={:.2} {gamut_str}
$c-light:    rgba({}, 1); // L={:.2} {gamut_str}
$c-neutral:  rgba({}, 1); // L={:.2} {gamut_str}
$c-dark:     rgba({}, 1); // L={:.2} {gamut_str}
$c-darker:   rgba({}, 1); // L={:.2} {gamut_str}
$c-darkest:  rgba({}, 1); // L={:.2} {gamut_str}"#,
                    neutrals.lightest,
                    neutrals.lightest.l,
                    neutrals.lighter,
                    neutrals.lighter.l,
                    neutrals.light,
                    neutrals.light.l,
                    neutrals.neutral,
                    neutrals.neutral.l,
                    neutrals.dark,
                    neutrals.dark.l,
                    neutrals.darker,
                    neutrals.darker.l,
                    neutrals.darkest,
                    neutrals.darkest.l,
                );

                let mut clipboard = Clipboard::new().unwrap();
                clipboard.set_text(colors).unwrap();
                return Ok(true);
            }

            // Cycle selected colors on tab.
            if key.kind == KeyEventKind::Press && key.code == KeyCode::Tab {
                self.colors_widget.active_color_block_index += 1;
                self.colors_widget.active_color_block_index %= 2;
                return Ok(true);
            }

            // Handle input events for the neutral color.
            if key.kind == KeyEventKind::Press && key.code == KeyCode::Right {
                if self.colors_widget.active_color_block_index == 0 {
                    self.colors_widget.base_neutral_color.h =
                        (self.colors_widget.base_neutral_color.h + 1.0) % 360.0;
                } else {
                    self.colors_widget.base_accent_hue_offset =
                        (self.colors_widget.base_accent_hue_offset + 1.0) % 360.0;
                }
            } else if key.kind == KeyEventKind::Press && key.code == KeyCode::Left {
                if self.colors_widget.active_color_block_index == 0 {
                    self.colors_widget.base_neutral_color.h =
                        (self.colors_widget.base_neutral_color.h - 1.0) % 360.0;
                } else {
                    self.colors_widget.base_accent_hue_offset =
                        (self.colors_widget.base_accent_hue_offset - 1.0) % 360.0;
                }
            } else if key.kind == KeyEventKind::Press && key.code == KeyCode::Up {
                if self.colors_widget.active_color_block_index == 0 {
                    self.colors_widget.base_neutral_color.c =
                        (self.colors_widget.base_neutral_color.c + NEUTRAL_CHROMA_STEP)
                            .min(NEUTRAL_MAX_CHROMA);
                } else {
                    self.colors_widget.base_accent_chromaticity =
                        (self.colors_widget.base_accent_chromaticity + NEUTRAL_CHROMA_STEP)
                            .min(NEUTRAL_MAX_CHROMA);
                }
            } else if key.kind == KeyEventKind::Press && key.code == KeyCode::Down {
                if self.colors_widget.active_color_block_index == 0 {
                    self.colors_widget.base_neutral_color.c =
                        (self.colors_widget.base_neutral_color.c - NEUTRAL_CHROMA_STEP).max(0.0);
                } else {
                    self.colors_widget.base_accent_chromaticity =
                        (self.colors_widget.base_accent_chromaticity - NEUTRAL_CHROMA_STEP)
                            .max(0.0);
                }
            }
        }

        Ok(true)
    }
}

/// Implement the Widget trait for &mut App so that it can be rendered
///
/// This is implemented on a mutable reference so that the app can update its state while it is
/// being rendered.
impl Widget for &mut App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        use Constraint::{Length, Min};
        let [top, colors, bottom] = Layout::vertical([Length(1), Min(0), Length(3)]).areas(area);
        let [_] = Layout::horizontal([Min(0)]).areas(top);
        let [instructions_area] = Layout::horizontal([Min(0)]).areas(bottom);

        let base_chroma = format!("{:0.3}", self.colors_widget.base_neutral_color.c);
        let base_hue: String = format!("{:0.2}", self.colors_widget.base_neutral_color.h);

        let g_label = if self.colors_widget.cmyk_gamut_fitting {
            "Disable"
        } else {
            "Enable"
        };

        Text::from(format!("\nQ: Quit | ↑↓: Chroma ({base_chroma}) | ←→: Hue ({base_hue}) | G: {g_label} CMYK Gamut Fitting | W: Copy SCSS")).centered().render(instructions_area, buf);

        let [colors] = Layout::horizontal([Min(0)])
            .flex(Flex::Center)
            .areas(colors);

        self.colors_widget.render(colors, buf);
    }
}

/// Widget impl for `ColorsWidget`
///
/// This is implemented on a mutable reference so that we can update the frame count and store a
/// cached version of the colors to render instead of recalculating them every frame.
impl Widget for &mut ColorsWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Generate the neutral colors.
        let mut neutrals = Neutrals::from_color_hue_adjusted(&self.base_neutral_color);
        if self.cmyk_gamut_fitting {
            neutrals = neutrals.to_cmyk_adjusted();
        }

        // Render a column for each neutral color.
        let neutral_colors = 7;
        let col_constraints = (0..neutral_colors).map(|_| Constraint::Min(9));

        // Render two rows of colors (one for neutrals, one for accents).
        let row_constraints = (0..2).map(|_| Constraint::Min(3));

        // Split the rendered area into vertical rows.
        let horizontal = Layout::horizontal(col_constraints).spacing(1);
        let vertical = Layout::vertical(row_constraints).spacing(1);
        let rows = vertical.split(area);

        // Split rows into cells.
        let mut cells = vec![];
        for (i, row) in rows.iter().enumerate() {
            // Wrap each row in a block.
            let mut block = Block::new();

            // Wrap a border around the active block.
            if i == self.active_color_block_index {
                let [r, g, b] = neutrals.neutral.to_srgb();
                let border_color =
                    Color::Rgb((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8);
                block = Block::bordered()
                    .border_type(BorderType::Thick)
                    .border_style(border_color);

            // Pad inactive blocks so that bordered and unbordered
            // blocks appear to have the same inner area.
            } else {
                block = block.padding(Padding::uniform(1));
            }
            let block_area = block.inner(*row);
            block.render(*row, buf);

            // Split the row into cells.
            cells.extend_from_slice(&horizontal.split(block_area));
        }

        // Convert neutrals into a list for rendering.
        let neutral = neutrals.neutral.clone();
        let neutrals = neutrals.into_iter().collect::<Vec<_>>();

        // Draw the neutral colors, in ascending lightness
        for (i, cell) in cells.iter().take(neutral_colors).enumerate() {
            render_color_block(*cell, buf, neutrals[i]);
        }

        // Draw accent colors, in ascending hue.
        for (i, cell) in cells.iter().skip(neutral_colors).enumerate() {
            // Derive the accent color.
            let mut color = neutral.clone();
            color.h =
                ((neutral.h + self.base_accent_hue_offset) + (ACCENT_HUE_STEP * i as f32)) % 360.0;
            color.c = self.base_accent_chromaticity;

            // Derive the tones of the accent color.
            let mut tones = Neutrals::from_color_hue_adjusted(&color);
            if self.cmyk_gamut_fitting {
                tones = tones.to_cmyk_adjusted();
            }

            // Split the cell into three regions.
            let [top, mid, bot] = Layout::vertical((0..3).map(|_| Constraint::Min(3)))
                .spacing(0)
                .areas(*cell);

            // Draw colors.
            render_color_block(top, buf, &tones.light);
            render_color_block(mid, buf, &tones.neutral);
            render_color_block(bot, buf, &tones.dark);
        }
    }
}

/// Fills `area` and `buff` with a block of `color`, overlaying
/// metadata about the color if there's enough space.
fn render_color_block(area: Rect, buff: &mut Buffer, color: &cate::tool::color::Color) {
    let fg_color = if color.l >= 0.5 {
        Color::Black
    } else {
        Color::White
    };

    let [r, g, b] = color.to_srgb();
    let bg_color = Color::Rgb((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8);

    // Draw hex code to wide areas.
    let mut paragraph = String::default();
    if area.width >= 11 {
        let hex = color.to_hex().to_ascii_uppercase();
        paragraph.push_str(&format!("\n  {hex}"));
    }

    // Draw LCH values to tall areas.
    if area.height >= 7 && area.width >= 12 {
        let bottom_padding = 3;
        let bottom_lines = 3;

        for _ in 0..(area.height - (bottom_padding + bottom_lines)) {
            paragraph.push('\n');
        }

        let l = format!("{:.2}", color.l);
        let c = format!("{:.3}", color.c);
        let h = format!("{:.2}", color.h);

        paragraph.push_str(&format!("\n  L {l}"));
        paragraph.push_str(&format!("\n  C {c}"));
        paragraph.push_str(&format!("\n  H {h}"));
    }

    Paragraph::new(paragraph)
        .fg(fg_color)
        .block(Block::new())
        .bg(bg_color)
        .render(area, buff);
}
