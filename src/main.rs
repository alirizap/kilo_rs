use std::io::stdout;

use anyhow::Result;
use crossterm::{
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
    }

    Ok(())
}
