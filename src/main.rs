use std::time::Duration;

use ratatui::{
    buffer::Buffer, crossterm::event::{self, Event, KeyCode, KeyEventKind}, layout::{Alignment, Constraint, Flex, Layout, Rect}, style::{Color, Style, Stylize}, widgets::{Block, Paragraph, Widget}, DefaultTerminal
};
use tui_textarea::TextArea;

const DEFAULT_NEUTRAL_COLOR: &str = "E1E7D4";

fn main() -> std::io::Result<()> {
    let terminal = ratatui::init();
    let app_result = App::default().run(terminal);
    ratatui::restore();
    app_result
}

#[derive(Debug, Default)]
struct App<'a> {
    /// A widget that displays the full range of RGB colors that can be displayed in the terminal.
    colors_widget: ColorsWidget<'a>,
}

/// A widget that displays the full range of RGB colors that can be displayed in the terminal.
///
/// This widget is animated and will change colors over time.
#[derive(Debug, Default)]
struct ColorsWidget<'a> {
    neutral_color: cate::Color,
    text_area: TextArea<'a>,
}

impl<'a> App<'a> {
    /// Run the app.
    ///
    /// This is the main event loop for the app.
    pub fn run(mut self, mut terminal: DefaultTerminal) -> std::io::Result<()> {
        self.colors_widget.neutral_color =
            cate::Color::try_from_hex(DEFAULT_NEUTRAL_COLOR.into()).unwrap();
        self.colors_widget
            .text_area
            .set_cursor_line_style(Style::default());
        self.colors_widget
            .text_area
            .set_alignment(Alignment::Center);
        self.colors_widget
            .text_area
            .set_placeholder_text(format!("{DEFAULT_NEUTRAL_COLOR} (q to quit, up/down to adjust hue, left/right to adjust chroma)"));

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
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press && key.code == KeyCode::Char('q') {
                    return Ok(false);
                }

                // Handle input events for the neutral color.
                if key.kind == KeyEventKind::Press && key.code == KeyCode::Up {
                    self.colors_widget.neutral_color.h = (self.colors_widget.neutral_color.h + 5.0) % 360.0;
                } else if key.kind == KeyEventKind::Press && key.code == KeyCode::Down {
                    self.colors_widget.neutral_color.h = (self.colors_widget.neutral_color.h - 5.0).max(0.0);
                } else if key.kind == KeyEventKind::Press && key.code == KeyCode::Right {
                    self.colors_widget.neutral_color.c = (self.colors_widget.neutral_color.c + 0.05).min(0.4);
                } else if key.kind == KeyEventKind::Press && key.code == KeyCode::Left {
                    self.colors_widget.neutral_color.c = (self.colors_widget.neutral_color.c - 0.05).max(0.0);
                } else if self.colors_widget.text_area.input(key) {
                    if let Ok(color) = cate::Color::try_from_hex(
                        self.colors_widget.text_area.lines()[0].clone().into(),
                    ) {
                        self.colors_widget.neutral_color = color;
                    }
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
impl<'a> Widget for &mut App<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        use Constraint::{Length, Min};
        let [top, colors] = Layout::vertical([Length(1), Min(0)]).areas(area);
        let [title] = Layout::horizontal([Min(0)]).areas(top);

        self.colors_widget.text_area.render(title, buf);

        // Text::from("Press q to quit").centered().render(title, buf);

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
impl<'a> Widget for &mut ColorsWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let cols = 8;
        let rows = 2;

        let col_constraints = (0..cols).map(|_| Constraint::Min(9));
        let row_constraints = (0..rows).map(|_| Constraint::Min(3));
        let horizontal = Layout::horizontal(col_constraints).spacing(1);
        let vertical = Layout::vertical(row_constraints).spacing(1);

        let rows = vertical.split(area);
        let cells = rows.iter().flat_map(|&row| horizontal.split(row).to_vec());

        // Generate the vector test colors.
        let neutrals_a = cate::Neutrals::from_color_hue_adjusted(&self.neutral_color);
        let neutrals_b = cate::Neutrals::from_color(&self.neutral_color);
        let neutrals_a = neutrals_a.into_iter().collect::<Vec<_>>();
        let neutrals_b = neutrals_b.into_iter().collect::<Vec<_>>();

        for (i, cell) in cells.enumerate() {
            let colors = if i < 8 { &neutrals_a } else { &neutrals_b };

            let color = colors[i % 8];
            let fg_color = if color.l >= 0.5 {
                Color::Black
            } else {
                Color::White
            };

            let (r, g, b) = color.to_rgb();
            let bg_color = Color::Rgb(r, g, b);

            let hex = color.to_hex();

            Paragraph::new(format!("\n  {}", hex.to_ascii_uppercase()))
                .fg(fg_color)
                .block(Block::new())
                .bg(bg_color)
                .render(cell, buf);
        }
    }
}
