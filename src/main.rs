use std::{
    fs::File,
    io::{stdout, BufRead, BufReader, Stdout, Write},
    time::{SystemTime, UNIX_EPOCH},
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
    QueueableCommand,
};

const KILO_RS_VERSION: &'static str = "0.1.1";
const KILO_RS_TAB_STOP: usize = 8;

struct Row {
    content: String,
    render: String,
    rsize: usize,
}

struct EditorConfig {
    stdout: Stdout,
    screen_rows: usize,
    screen_cols: usize,
    cx: usize,
    cy: usize,
    rx: usize,
    col_off: usize,
    row_off: usize,
    row: Vec<Row>,
    filename: Option<String>,
    status_msg: String,
    status_msg_time: u64,
}

impl EditorConfig {
    fn new() -> Result<Self> {
        let (screen_cols, screen_rows) = size()?;
        Ok(EditorConfig {
            stdout: stdout(),
            screen_rows: (screen_rows - 2) as usize,
            screen_cols: screen_cols as usize,
            cx: 0,
            cy: 0,
            rx: 0,
            col_off: 0,
            row_off: 0,
            row: Vec::new(),
            filename: None,
            status_msg: String::new(),
            status_msg_time: 0,
        })
    }
}

// Terminal

fn die(err: Error) -> ! {
    disable_raw_mode().unwrap();
    execute!(
        stdout(),
        LeaveAlternateScreen,
        cursor::SetCursorStyle::DefaultUserShape
    )
    .unwrap();
    eprintln!("{err}");
    std::process::exit(1);
}

// Row operations

fn editor_row_cx_to_rx(row: &Row, cx: usize) -> usize {
    let mut rx = 0;
    for c in row.content.chars().take(cx) {
        if c == '\t' {
            rx += (KILO_RS_TAB_STOP - 1) - (rx % KILO_RS_TAB_STOP);
        }
        rx += 1;
    }
    rx
}

fn editor_update_row(row: &mut Row) {
    row.render.clear();
    let mut idx = 0;
    for c in row.content.chars() {
        if c == '\t' {
            row.render.push(' ');
            idx += 1;
            while idx % KILO_RS_TAB_STOP != 0 {
                row.render.push(' ');
                idx += 1;
            }
        } else {
            row.render.push(c);
            idx += 1;
        }
    }
    row.rsize = idx;
}

fn editor_append_row(config: &mut EditorConfig, s: &str) {
    let mut row = Row {
        content: s.to_string(),
        render: String::new(),
        rsize: 0,
    };
    editor_update_row(&mut row);
    config.row.push(row);
}

// File I/O

fn editor_open(config: &mut EditorConfig, filename: String) {
    config.filename = Some(filename.to_string());
    let reader = BufReader::new(File::open(filename).unwrap_or_else(|err| die(err.into())));
    for line in reader.lines() {
        let line = line.unwrap_or_else(|err| die(err.into()));
        editor_append_row(config, &line);
    }
}

// Output

fn editor_scroll(config: &mut EditorConfig) {
    config.rx = if config.cy < config.row.len() {
        let row = &config.row[config.cy];
        editor_row_cx_to_rx(row, config.cx)
    } else {
        0
    };

    if config.cy < config.row_off {
        config.row_off = config.cy;
    }
    if config.cy >= config.row_off + config.screen_rows {
        config.row_off = config.cy - config.screen_rows + 1;
    }
    if config.rx < config.col_off {
        config.col_off = config.rx;
    }
    if config.rx >= config.col_off + config.screen_cols {
        config.col_off = config.rx - config.screen_cols + 1;
    }
}

fn editor_draw_rows(config: &mut EditorConfig, buf: &mut String) -> Result<()> {
    for y in 0..config.screen_rows {
        let file_row = y + config.row_off;
        if file_row >= config.row.len() {
            if config.row.len() == 0 && y == config.screen_rows / 3 {
                let mut welcome = format!("Kilo-rs editor -- version {KILO_RS_VERSION}");
                if welcome.len() > config.screen_cols {
                    welcome.truncate(config.screen_cols);
                }
                let mut padding = (config.screen_cols - welcome.len()) / 2;
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
            let mut len = config.row[file_row].rsize.saturating_sub(config.col_off);
            if len > config.screen_cols {
                len = config.screen_cols;
            }

            let end = len + config.col_off;
            buf.push_str(&config.row[file_row].render[config.col_off..end]);
        }

        buf.push_str("\r\n");
    }
    Ok(())
}

fn editor_draw_statusbar(config: &EditorConfig, buf: &mut String) {
    buf.push_str("\x1b[7m");
    let mut status = format!(
        "{} - {} lines",
        if let Some(file) = &config.filename {
            file.clone()
        } else {
            "[No Name]".to_string()
        },
        config.row.len()
    );
    let rstatus = format!("{}/{}", config.cy + 1, config.row.len());
    let mut len = status.len();
    if status.len() > config.screen_cols {
        len = config.screen_cols;
    }
    let rlen = rstatus.len();
    status.truncate(len);
    buf.push_str(&status);
    while len < config.screen_cols {
        if config.screen_cols - len == rlen {
            buf.push_str(&rstatus);
            break;
        }
        buf.push(' ');
        len += 1;
    }
    buf.push_str("\x1b[m");
    buf.push_str("\r\n");
}

fn editor_draw_messagebar(config: &mut EditorConfig, buf: &mut String) -> Result<()> {
    buf.push_str("\x1b[K");
    let msglen = if config.status_msg.len() > config.screen_cols {
        config.screen_cols
    } else {
        config.status_msg.len()
    };
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    if msglen > 0 && (now - config.status_msg_time < 5) {
        buf.push_str(&config.status_msg[..msglen]);
    }
    Ok(())
}

fn editor_refresh_screen(config: &mut EditorConfig) -> Result<()> {
    editor_scroll(config);

    let mut buf = String::new();

    config.stdout.queue(cursor::Hide)?;
    config.stdout.queue(Clear(ClearType::All))?;
    config.stdout.queue(cursor::MoveTo(0, 0))?;

    editor_draw_rows(config, &mut buf)?;
    editor_draw_statusbar(config, &mut buf);
    editor_draw_messagebar(config, &mut buf)?;

    config.stdout.queue(style::Print(buf))?;
    config.stdout.queue(cursor::MoveTo(
        (config.rx - config.col_off) as u16,
        (config.cy - config.row_off) as u16,
    ))?;
    config.stdout.queue(cursor::Show)?;
    config.stdout.flush()?;
    Ok(())
}

fn editor_set_status_msg(config: &mut EditorConfig, msg: String) -> Result<()> {
    config.status_msg = msg;
    config.status_msg_time = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    Ok(())
}

// Input

fn editor_move_cursor(config: &mut EditorConfig, key: KeyCode) {
    let row = if config.cy >= config.row.len() {
        None
    } else {
        Some(&config.row[config.cy])
    };
    match key {
        KeyCode::Left => {
            if config.cx != 0 {
                config.cx -= 1;
            } else if config.cy > 0 {
                config.cy -= 1;
                config.cx = config.row[config.cy].content.len();
            }
        }
        KeyCode::Right => {
            if row.is_some_and(|r| r.content.len() > config.cx) {
                config.cx += 1;
            } else if row.is_some_and(|r| r.content.len() == config.cx) {
                config.cy += 1;
                config.cx = 0;
            }
        }
        KeyCode::Up => {
            if config.cy != 0 {
                config.cy -= 1;
            }
        }
        KeyCode::Down => {
            if config.row.len() > config.cy.into() {
                config.cy += 1;
            }
        }
        _ => todo!("Wait What!?"),
    }

    let row = if config.cy >= config.row.len() {
        None
    } else {
        Some(&config.row[config.cy])
    };
    if row.is_some_and(|r| config.cx > r.content.len()) {
        config.cx = row.unwrap().content.len();
    }
}

fn editor_process_keypress(config: &mut EditorConfig) -> Result<()> {
    let event = read()?;
    if let Event::Key(key) = event {
        match key.code {
            KeyCode::Right | KeyCode::Left | KeyCode::Up | KeyCode::Down => {
                editor_move_cursor(config, key.code)
            }
            KeyCode::PageUp | KeyCode::PageDown => {
                if key.code == KeyCode::PageUp {
                    config.cy = config.row_off;
                } else {
                    config.cy = config.row_off + config.screen_rows - 1;
                    if config.cy > config.row.len() {
                        config.cy = config.row.len();
                    }
                }

                let mut times = config.screen_rows;
                while times != 0 {
                    editor_move_cursor(
                        config,
                        if key.code == KeyCode::PageUp {
                            KeyCode::Up
                        } else {
                            KeyCode::Down
                        },
                    );
                    times -= 1;
                }
            }
            KeyCode::Home => config.cx = 0,
            KeyCode::End if config.cy < config.row.len() => {
                config.cx = config.row[config.cy].content.len()
            }
            KeyCode::Char('q') if key.modifiers == KeyModifiers::CONTROL => {
                disable_raw_mode().unwrap();
                execute!(
                    config.stdout,
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

// Main

fn main() -> Result<()> {
    let mut config = EditorConfig::new()?;
    execute!(
        stdout(),
        EnterAlternateScreen,
        cursor::SetCursorStyle::SteadyBlock
    )?;
    enable_raw_mode()?;
    let filename = std::env::args().nth(1);
    if let Some(filename) = filename {
        editor_open(&mut config, filename);
    }
    editor_set_status_msg(&mut config, "HELP: Ctrl-Q = quit".to_string())
        .unwrap_or_else(|err| die(err));
    loop {
        editor_refresh_screen(&mut config).unwrap_or_else(|err| die(err));
        editor_process_keypress(&mut config).unwrap_or_else(|err| die(err));
    }
}
