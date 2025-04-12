use std::io::stdout;

use anyhow::Result;
use crossterm::{
    event::{read, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{
        disable_raw_mode, enable_raw_mode, size, EnterAlternateScreen, LeaveAlternateScreen,
    },
};

struct EditorConfig {
    screen_rows: u16,
    screen_cols: u16,
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
            },
        }
    }

    fn run(&mut self) -> Result<()> {
        loop {
            self.process_keypress()?;
        }
    }

    fn process_keypress(&self) -> Result<()> {
        let event = read()?;
        if let Event::Key(key) = event {
            match key.code {
                KeyCode::Char('q') if key.modifiers == KeyModifiers::CONTROL => {
                    disable_raw_mode().unwrap();
                    execute!(stdout(), LeaveAlternateScreen).unwrap();
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
