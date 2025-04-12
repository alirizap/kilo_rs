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
    cx: u16,
    cy: u16,
}

struct Editor {
    cg: EditorConfig,
    sc: Stdout,
}

impl Editor {
    fn new(screen_rows: u16, screen_cols: u16) -> Self {
        Self {
            cg: EditorConfig {
                screen_rows,
                screen_cols,
                cx: 0,
                cy: 0,
            },
            sc: stdout(),
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
        for y in 0..self.cg.screen_rows {
            if y == self.cg.screen_rows / 3 {
                let mut welcome = format!("Kilo-rs editor -- version {KILO_RS_VERSION}");
                if welcome.len() > self.cg.screen_cols.into() {
                    welcome.truncate(self.cg.screen_cols.into());
                }
                let mut padding = (self.cg.screen_cols as usize - welcome.len()) / 2;
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

            if y < self.cg.screen_rows - 1 {
                buf.push_str("\r\n");
            }
        }
        Ok(())
    }

    fn refresh_screen(&mut self) -> Result<()> {
        let mut buf = String::new();

        self.sc.queue(cursor::Hide)?;
        self.sc.queue(Clear(ClearType::All))?;
        self.sc.queue(cursor::MoveTo(0, 0))?;

        self.draw_rows(&mut buf)?;

        self.sc.queue(style::Print(buf))?;
        self.sc.queue(cursor::MoveTo(self.cg.cx + 1, self.cg.cy))?;
        self.sc.queue(cursor::Show)?;
        self.sc.flush()?;
        Ok(())
    }

    // Input

    fn move_cursor(&mut self, key: KeyCode) {
        match key {
            KeyCode::Left => {
                if self.cg.cx != 0 {
                    self.cg.cx -= 1;
                }
            }
            KeyCode::Right => {
                if self.cg.cx != self.cg.screen_cols - 1 {
                    self.cg.cx += 1;
                }
            }
            KeyCode::Up => {
                if self.cg.cy != 0 {
                    self.cg.cy -= 1;
                }
            }
            KeyCode::Down => {
                if self.cg.cy != self.cg.screen_rows {
                    self.cg.cy += 1;
                }
            }
            _ => todo!("Wait What!?"),
        }
    }

    fn process_keypress(&mut self) -> Result<()> {
        let event = read()?;
        if let Event::Key(key) = event {
            match key.code {
                KeyCode::Right | KeyCode::Left | KeyCode::Up | KeyCode::Down => {
                    self.move_cursor(key.code)
                }
                KeyCode::PageUp | KeyCode::PageDown => {
                    let mut times = self.cg.screen_rows;
                    while times != 0 {
                        self.move_cursor(if key.code == KeyCode::PageUp {
                            KeyCode::Up
                        } else {
                            KeyCode::Down
                        });
                        times -= 1;
                    }
                }
                KeyCode::Home => self.cg.cx = 0,
                KeyCode::End => self.cg.cx = self.cg.screen_cols - 1,
                KeyCode::Char('q') if key.modifiers == KeyModifiers::CONTROL => {
                    disable_raw_mode().unwrap();
                    execute!(self.sc, LeaveAlternateScreen).unwrap();
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
