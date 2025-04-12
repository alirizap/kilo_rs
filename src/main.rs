use std::io::{stdout, Stdout, Write};

use anyhow::Result;
use crossterm::{
    cursor,
    event::{read, Event, KeyCode, KeyModifiers},
    execute, style,
    terminal::{
        disable_raw_mode, enable_raw_mode, size, Clear, ClearType, EnterAlternateScreen,
        LeaveAlternateScreen,
    },
    QueueableCommand,
};

const KILO_RS_VERSION: &'static str = "0.1.1";

struct EditorConfig {
    screen_rows: u16,
    screen_cols: u16,
    sc: Stdout,
}

struct Editor {
    config: EditorConfig,
}

impl Editor {
    fn new(screen_rows: u16, screen_cols: u16) -> Self {
        Self {
            config: EditorConfig {
                screen_rows,
                screen_cols,
                sc: stdout(),
            },
        }
    }

    fn run(&mut self) -> Result<()> {
        loop {
            self.refresh_screen()?;
            self.process_keypress()?;
        }
    }

    // Output

    fn draw_rows(&mut self, buf: &mut String) -> Result<()> {
        for y in 0..self.config.screen_rows {
            if y == self.config.screen_rows / 3 {
                let mut welcome = format!("Kilo-rs editor -- version {KILO_RS_VERSION}");
                if welcome.len() > self.config.screen_cols.into() {
                    welcome.truncate(self.config.screen_cols.into());
                }
                let mut padding = (self.config.screen_cols as usize - welcome.len()) / 2;
                if padding > 0 {
                    buf.push('~');
                    padding -= 1
                }

                while padding != 0 {
                    buf.push(' ');
                    padding -= 1;
                }
                buf.push_str(&welcome);
            } else {
                buf.push('~');
            }

            if y < self.config.screen_rows - 1 {
                buf.push_str("\r\n");
            }
        }
        Ok(())
    }

    fn refresh_screen(&mut self) -> Result<()> {
        let mut buf = String::new();

        self.config.sc.queue(cursor::Hide)?;
        self.config.sc.queue(Clear(ClearType::All))?;
        self.config.sc.queue(cursor::MoveTo(0, 0))?;

        self.draw_rows(&mut buf)?;

        self.config.sc.queue(style::Print(buf))?;
        self.config.sc.queue(cursor::MoveTo(0, 0))?;
        self.config.sc.queue(cursor::Show)?;
        self.config.sc.flush()?;
        Ok(())
    }

    // Input

    fn process_keypress(&mut self) -> Result<()> {
        let event = read()?;
        if let Event::Key(key) = event {
            match key.code {
                KeyCode::Char('q') if key.modifiers == KeyModifiers::CONTROL => {
                    disable_raw_mode().unwrap();
                    execute!(self.config.sc, LeaveAlternateScreen).unwrap();
                    std::process::exit(0);
                }
                _ => {}
            }
        }
        Ok(())
    }
}

fn main() -> Result<()> {
    let (screen_cols, screen_rows) = size()?;
    execute!(stdout(), EnterAlternateScreen)?;
    enable_raw_mode()?;
    let mut editor = Editor::new(screen_rows, screen_cols);
    if let Err(e) = editor.run() {
        eprint!("{e}");
        disable_raw_mode().unwrap();
        execute!(stdout(), LeaveAlternateScreen).unwrap();
        std::process::exit(1);
    }

    Ok(())
}
