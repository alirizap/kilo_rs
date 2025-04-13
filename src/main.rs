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
const KILO_RS_TAB_STOP: usize = 8;

struct Row {
    content: String,
    render: String,
    rsize: usize,
}

impl Row {
    fn update(&mut self) {
        self.render.clear();
        let mut idx = 0;
        for c in self.content.chars() {
            if c == '\t' {
                self.render.push(' ');
                idx += 1;
                while idx % KILO_RS_TAB_STOP != 0 {
                    self.render.push(' ');
                    idx += 1;
                }
            } else {
                self.render.push(c);
                idx += 1;
            }
        }
        self.rsize = idx;
    }
}

struct EditorConfig {
    screen_rows: usize,
    screen_cols: usize,
    cx: usize,
    cy: usize,
    rx: usize,
    col_off: usize,
    row_off: usize,
    row: Vec<Row>,
}

struct Editor {
    cfg: EditorConfig,
    sc: Stdout,
    file: Option<String>,
}

impl Editor {
    fn new(screen_rows: u16, screen_cols: u16, filename: Option<String>) -> Self {
        Self {
            cfg: EditorConfig {
                screen_rows: screen_rows as usize,
                screen_cols: screen_cols as usize,
                cx: 0,
                cy: 0,
                rx: 0,
                col_off: 0,
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
        self.cfg.rx = if self.cfg.cy < self.cfg.row.len() {
            let row = &self.cfg.row[self.cfg.cy];
            self.cx_to_rx(row)
        } else {
            0
        };

        if self.cfg.cy < self.cfg.row_off {
            self.cfg.row_off = self.cfg.cy;
        }
        if self.cfg.cy >= self.cfg.row_off + self.cfg.screen_rows {
            self.cfg.row_off = self.cfg.cy - self.cfg.screen_rows + 1;
        }
        if self.cfg.rx < self.cfg.col_off {
            self.cfg.col_off = self.cfg.rx;
        }
        if self.cfg.rx >= self.cfg.col_off + self.cfg.screen_cols {
            self.cfg.col_off = self.cfg.rx - self.cfg.screen_cols + 1;
        }
    }

    fn draw_rows(&mut self, buf: &mut String) -> Result<()> {
        for y in 0..self.cfg.screen_rows {
            let file_row = y + self.cfg.row_off;
            if file_row >= self.cfg.row.len() {
                if self.cfg.row.len() == 0 && y == self.cfg.screen_rows / 3 {
                    let mut welcome = format!("Kilo-rs editor -- version {KILO_RS_VERSION}");
                    if welcome.len() > self.cfg.screen_cols.into() {
                        welcome.truncate(self.cfg.screen_cols.into());
                    }
                    let mut padding = (self.cfg.screen_cols as usize - welcome.len()) / 2;
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
                let mut len = self.cfg.row[file_row]
                    .rsize
                    .saturating_sub(self.cfg.col_off);
                if len > self.cfg.screen_cols {
                    len = self.cfg.screen_cols;
                }

                let end = len + self.cfg.col_off;
                buf.push_str(&self.cfg.row[file_row].render[self.cfg.col_off..end]);
            }

            if y < self.cfg.screen_rows - 1 {
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
            (self.cfg.rx - self.cfg.col_off) as u16,
            (self.cfg.cy - self.cfg.row_off) as u16,
        ))?;
        self.sc.queue(cursor::Show)?;
        self.sc.flush()?;
        Ok(())
    }

    // Row operations

    fn cx_to_rx(&self, row: &Row) -> usize {
        let mut rx = 0;
        for c in row.content.chars().take(self.cfg.cx) {
            if c == '\t' {
                rx += (KILO_RS_TAB_STOP - 1) - (rx % KILO_RS_TAB_STOP);
            }
            rx += 1;
        }
        rx
    }

    // Input

    fn move_cursor(&mut self, key: KeyCode) {
        let row = if self.cfg.cy >= self.cfg.row.len() {
            None
        } else {
            Some(&self.cfg.row[self.cfg.cy])
        };

        match key {
            KeyCode::Left => {
                if self.cfg.cx != 0 {
                    self.cfg.cx -= 1;
                } else if self.cfg.cy > 0 {
                    self.cfg.cy -= 1;
                    self.cfg.cx = self.cfg.row[self.cfg.cy].content.len();
                }
            }
            KeyCode::Right => {
                if row.is_some_and(|r| r.content.len() > self.cfg.cx) {
                    self.cfg.cx += 1;
                } else if row.is_some_and(|r| r.content.len() == self.cfg.cx) {
                    self.cfg.cy += 1;
                    self.cfg.cx = 0;
                }
            }
            KeyCode::Up => {
                if self.cfg.cy != 0 {
                    self.cfg.cy -= 1;
                }
            }
            KeyCode::Down => {
                if self.cfg.row.len() > self.cfg.cy.into() {
                    self.cfg.cy += 1;
                }
            }
            _ => todo!("Wait What!?"),
        }

        let row = if self.cfg.cy >= self.cfg.row.len() {
            None
        } else {
            Some(&self.cfg.row[self.cfg.cy])
        };
        if row.is_some_and(|r| self.cfg.cx > r.content.len()) {
            self.cfg.cx = row.unwrap().content.len();
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
                    let mut times = self.cfg.screen_rows;
                    while times != 0 {
                        self.move_cursor(if key.code == KeyCode::PageUp {
                            KeyCode::Up
                        } else {
                            KeyCode::Down
                        });
                        times -= 1;
                    }
                }
                KeyCode::Home => self.cfg.cx = 0,
                KeyCode::End => self.cfg.cx = self.cfg.screen_cols - 1,
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
                let mut row = Row {
                    content: line,
                    render: String::new(),
                    rsize: 0,
                };
                row.update();
                self.cfg.row.push(row);
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
