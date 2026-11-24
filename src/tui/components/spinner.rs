use super::Component;
use ratatui::{Frame, layout::Rect, widgets::Paragraph};
use std::time::Duration;

const FRAMES: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
const INTERVAL: Duration = Duration::from_millis(100);

#[derive(Default)]
pub struct Spinner {
    frame: usize,
    elapsed: Duration,
}

impl Component for Spinner {
    fn update(&mut self, elapsed: Duration) {
        self.elapsed += elapsed;
        while self.elapsed >= INTERVAL {
            self.elapsed -= INTERVAL;
            self.frame = (self.frame + 1) % FRAMES.len();
        }
    }

    fn render(&self, f: &mut Frame, area: Rect) {
        f.render_widget(Paragraph::new(FRAMES[self.frame].to_string()), area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advances_and_wraps() {
        let mut spinner = Spinner::default();
        spinner.update(INTERVAL);
        assert_eq!(spinner.frame, 1);
        spinner.update(INTERVAL * (FRAMES.len() as u32 - 1));
        assert_eq!(spinner.frame, 0);
    }

    #[test]
    fn preserves_partial_ticks_and_handles_delays() {
        let mut spinner = Spinner::default();
        spinner.update(Duration::from_millis(60));
        assert_eq!(spinner.frame, 0);
        spinner.update(Duration::from_millis(60));
        assert_eq!(spinner.frame, 1);
        spinner.update(Duration::from_millis(280));
        assert_eq!(spinner.frame, 4);
        assert_eq!(spinner.elapsed, Duration::ZERO);
    }
}
