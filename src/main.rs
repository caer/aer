use std::time::Duration;

use ratatui::{
    DefaultTerminal,
    buffer::Buffer,
    crossterm::event::{self, Event, KeyCode, KeyEventKind},
    layout::{Constraint, Flex, Layout, Rect},
    style::{Color, Stylize},
    text::Text,
    widgets::{Block, Paragraph, Widget},
};

fn test_neutrals() -> (cate::Neutrals, cate::Neutrals) {
    let color = cate::Color::from_hex("E1E7D4".into());
    (
        cate::Neutrals::from_color_hue_adjusted(&color),
        cate::Neutrals::from_color(&color),
    )
}

fn main() -> std::io::Result<()> {
    let terminal = ratatui::init();
    let app_result = App::default().run(terminal);
    ratatui::restore();
    app_result
}

#[derive(Debug, Default)]
struct App {
    /// A widget that displays the full range of RGB colors that can be displayed in the terminal.
    colors_widget: ColorsWidget,
}

/// A widget that displays the full range of RGB colors that can be displayed in the terminal.
///
/// This widget is animated and will change colors over time.
#[derive(Debug, Default)]
struct ColorsWidget {}

impl App {
    /// Run the app.
    ///
    /// This is the main event loop for the app.
    pub fn run(mut self, mut terminal: DefaultTerminal) -> std::io::Result<()> {
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
                };
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
        let [top, colors] = Layout::vertical([Length(1), Min(0)]).areas(area);
        let [title] = Layout::horizontal([Min(0)]).areas(top);
        Text::from("Press q to quit").centered().render(title, buf);
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
        let cols = 8;
        let rows = 2;

        let col_constraints = (0..cols).map(|_| Constraint::Min(9));
        let row_constraints = (0..rows).map(|_| Constraint::Min(3));
        let horizontal = Layout::horizontal(col_constraints).spacing(1);
        let vertical = Layout::vertical(row_constraints).spacing(1);

        let rows = vertical.split(area);
        let cells = rows.iter().flat_map(|&row| horizontal.split(row).to_vec());

        // Generate the vector test colors.
        let (neutrals_a, neutrals_b) = test_neutrals();
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
