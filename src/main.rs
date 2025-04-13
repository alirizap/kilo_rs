use std::{
    fs::File,
    io::{stdout, BufRead, BufReader, Stdout, Write},
};

use anyhow::{Error, Result};
use crossterm::{
    cursor,
    event::{read, Event, KeyCode, KeyModifiers},
    execute, style,
    terminal::{
        disable_raw_mode, enable_raw_mode, size, Clear, ClearType, EnterAlternateScreen,
        LeaveAlternateScreen,
    },
    ExecutableCommand, QueueableCommand,
};

const KILO_RS_VERSION: &'static str = "0.1.1";

struct EditorConfig {
    screen_rows: u16,
    screen_cols: u16,
    cx: u16,
    cy: u16,
    row_off: usize,
    row: Vec<String>,
}

struct Editor {
    cg: EditorConfig,
    sc: Stdout,
    file: Option<String>,
}

impl Editor {
    fn new(screen_rows: u16, screen_cols: u16, filename: Option<String>) -> Self {
        Self {
            cg: EditorConfig {
                screen_rows,
                screen_cols,
                cx: 0,
                cy: 0,
                row_off: 0,
                row: Vec::new(),
            },
            sc: stdout(),
            file: filename,
        }
    }

    fn run(&mut self) -> ! {
        self.open();
        self.sc
            .execute(cursor::SetCursorStyle::SteadyBlock)
            .unwrap();
        loop {
            self.refresh_screen().unwrap_or_else(|err| self.die(err));
            self.process_keypress().unwrap_or_else(|err| self.die(err));
        }
    }

    fn die(&mut self, err: Error) -> ! {
        disable_raw_mode().unwrap();
        execute!(
            self.sc,
            LeaveAlternateScreen,
            cursor::SetCursorStyle::DefaultUserShape
        )
        .unwrap();
        eprintln!("{err}");
        std::process::exit(1);
    }

    // Output

    fn scroll(&mut self) {
        if self.cg.row_off > self.cg.cy.into() {
            self.cg.row_off = self.cg.cy.into();
        }
        if self.cg.row_off + self.cg.screen_rows as usize <= self.cg.cy.into() {
            self.cg.row_off = (self.cg.cy - self.cg.screen_rows + 1).into();
        }
    }

    fn draw_rows(&mut self, buf: &mut String) -> Result<()> {
        for y in 0..self.cg.screen_rows {
            let file_row = y as usize + self.cg.row_off;
            if file_row >= self.cg.row.len() {
                if self.cg.row.len() == 0 && y == self.cg.screen_rows / 3 {
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
            } else {
                // truncate a row if its len greater then screen columns
                if self.cg.row[file_row].len() > self.cg.screen_cols as usize {
                    self.cg.row[file_row].truncate(self.cg.screen_cols as usize);
                }
                buf.push_str(&self.cg.row[file_row]);
            }

            if y < self.cg.screen_rows - 1 {
                buf.push_str("\r\n");
            }
        }
        Ok(())
    }

    fn refresh_screen(&mut self) -> Result<()> {
        self.scroll();

        let mut buf = String::new();

        self.sc.queue(cursor::Hide)?;
        self.sc.queue(Clear(ClearType::All))?;
        self.sc.queue(cursor::MoveTo(0, 0))?;

        self.draw_rows(&mut buf)?;

        self.sc.queue(style::Print(buf))?;
        self.sc.queue(cursor::MoveTo(
            self.cg.cx,
            self.cg.cy - self.cg.row_off as u16,
        ))?;
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
                if self.cg.row.len() > self.cg.cy.into() {
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
                    execute!(
                        self.sc,
                        LeaveAlternateScreen,
                        cursor::SetCursorStyle::DefaultUserShape
                    )
                    .unwrap();
                    std::process::exit(0);
                }
                _ => {}
            }
        }
        Ok(())
    }

    // File I/O

    fn open(&mut self) {
        if let Some(file) = &self.file {
            let reader =
                BufReader::new(File::open(file).unwrap_or_else(|err| self.die(err.into())));
            for line in reader.lines() {
                let line = line.unwrap_or_else(|err| self.die(err.into()));
                self.cg.row.push(line);
            }
        }
    }
}

fn main() -> Result<()> {
    let (screen_cols, screen_rows) = size()?;
    execute!(stdout(), EnterAlternateScreen)?;
    enable_raw_mode()?;
    let filename = std::env::args().nth(1);

    let mut editor = Editor::new(screen_rows, screen_cols, filename);
    editor.run();
}
