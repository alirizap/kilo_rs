use anyhow::Result;
use crossterm::terminal::size;

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
    let mut editor = Editor::new(screen_rows, screen_cols);
    editor.run()?;

    Ok(())
}
