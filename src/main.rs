use std::{
    fmt::Write,
    fs::{File, OpenOptions},
    io::{stdout, BufRead, BufReader, Stdout, Write as _},
    path::Path,
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

const KILO_RS_VERSION: &str = "0.1.1";
const KILO_RS_TAB_STOP: usize = 8;
const KILO_RS_QUIT_TIMES: u8 = 3;
const HL_HIGHLIGHT_NUMBERS: u32 = 1 << 0;

type Callback = Box<dyn Fn(&mut EditorConfig, &str, KeyCode)>;

#[derive(Clone, Copy, PartialEq)]
enum Highlight {
    Normal,
    Number,
}

impl Highlight {
    fn to_color(self) -> u8 {
        match self {
            Self::Number => 31,
            _ => 37,
        }
    }
}

#[derive(Clone, Copy)]
struct Syntax {
    filetype: &'static str,
    filematch: &'static [&'static str],
    flags: u32,
}

const HLDB: [Syntax; 1] = [Syntax {
    filetype: "rust",
    filematch: &["rs"],
    flags: HL_HIGHLIGHT_NUMBERS,
}];

struct Row {
    content: String,
    render: String,
    rsize: usize,
    hl: Vec<Highlight>,
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
    dirty: bool,
    syntax: Option<Syntax>,
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
            dirty: false,
            syntax: None,
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

// Syntax highlighting

fn is_separator(c: char) -> bool {
    c.is_ascii_punctuation() || c.is_ascii_whitespace() || c == '\0'
}

fn update_syntax(syntax: Option<Syntax>, row: &mut Row) {
    row.hl.resize(row.rsize, Highlight::Normal);

    if syntax.is_none() {
        return;
    }

    let mut prev_sep = true;
    let mut i = 0;
    while i < row.rsize {
        let c = row.render.chars().nth(i).unwrap();
        let prev_hl = if i > 0 {
            row.hl[i - 1]
        } else {
            Highlight::Normal
        };

        if syntax.unwrap().flags & HL_HIGHLIGHT_NUMBERS != 0 {
            if (c.is_ascii_digit() && (prev_sep || prev_hl == Highlight::Number))
                || (c == '.' && prev_hl == Highlight::Number)
            {
                row.hl[i] = Highlight::Number;
                i += 1;
                prev_sep = false;
                continue;
            }
        }

        prev_sep = is_separator(c);
        i += 1;
    }
}

fn select_syntax_highlight(config: &mut EditorConfig) {
    if config.filename.is_none() {
        return;
    }

    if let Some(ext) = Path::new(config.filename.as_ref().unwrap().as_str()).extension() {
        let ext = ext.to_str().unwrap();
        for s in HLDB {
            for fm in s.filematch {
                if &ext == fm {
                    config.syntax = Some(s);
                    for row in config.row.iter_mut() {
                        update_syntax(config.syntax, row);
                    }
                    return;
                }
            }
        }
    }
}

// Row operations

fn row_cx_to_rx(row: &Row, cx: usize) -> usize {
    let mut rx = 0;
    for c in row.content.chars().take(cx) {
        if c == '\t' {
            rx += (KILO_RS_TAB_STOP - 1) - (rx % KILO_RS_TAB_STOP);
        }
        rx += 1;
    }
    rx
}

fn row_rx_to_cx(row: &Row, rx: usize) -> usize {
    let mut cur_rx = 0;
    let mut ret_cx = 0;
    for (cx, c) in row.content.chars().enumerate() {
        if c == '\t' {
            cur_rx += (KILO_RS_TAB_STOP - 1) - (cur_rx % KILO_RS_TAB_STOP);
        }
        ret_cx = cx;
        cur_rx += 1;
        if cur_rx > rx {
            return cx;
        }
    }
    ret_cx
}

fn update_row(syntax: Option<Syntax>, row: &mut Row) {
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
    update_syntax(syntax, row);
}

fn insert_row(config: &mut EditorConfig, at: usize, s: &str) {
    if at > config.row.len() {
        return;
    }
    let row = Row {
        content: s.to_string(),
        render: String::new(),
        rsize: 0,
        hl: Vec::new(),
    };
    config.row.insert(at, row);
    update_row(config.syntax, &mut config.row[at]);
    config.dirty = true;
}

fn del_row(config: &mut EditorConfig, at: usize) {
    if at >= config.row.len() {
        return;
    }
    config.row.remove(at);
    config.dirty = true;
}

fn row_insert_char(syntax: Option<Syntax>, row: &mut Row, at: usize, c: char) {
    let at = if at > row.content.len() {
        row.content.len()
    } else {
        at
    };
    row.content.insert(at, c);
    update_row(syntax, row);
}

fn row_append_string(syntax: Option<Syntax>, row: &mut Row, s: &str) {
    row.content.push_str(s);
    update_row(syntax, row);
}

fn row_del_char(syntax: Option<Syntax>, row: &mut Row, at: usize) {
    if at >= row.content.len() {
        return;
    }
    row.content.remove(at);
    update_row(syntax, row);
}

// editor operations

fn insert_char(config: &mut EditorConfig, c: char) {
    if config.cy == config.row.len() {
        insert_row(config, config.row.len(), "");
    }
    row_insert_char(config.syntax, &mut config.row[config.cy], config.cx, c);
    config.cx += 1;
    config.dirty = true;
}

fn insert_newline(config: &mut EditorConfig) {
    if config.cx == 0 {
        insert_row(config, config.cy, "");
    } else {
        let content = config.row[config.cy].content.clone();
        insert_row(config, config.cy + 1, &content[config.cx..]);
        config.row[config.cy].content.truncate(config.cx);
        update_row(config.syntax, &mut config.row[config.cy]);
    }
    config.cy += 1;
    config.cx = 0;
    config.dirty = true;
}

fn del_char(config: &mut EditorConfig) {
    if config.cy == config.row.len() {
        return;
    }

    if config.cx == 0 && config.cy == 0 {
        return;
    }

    if config.cx > 0 {
        let row = &mut config.row[config.cy];
        row_del_char(config.syntax, row, config.cx - 1);
        config.cx -= 1;
        config.dirty = true;
    } else {
        config.cx = config.row[config.cy - 1].content.len();
        let content = config.row[config.cy].content.clone();
        row_append_string(config.syntax, &mut config.row[config.cy - 1], &content);
        del_row(config, config.cy);
        config.cy -= 1;
        config.dirty = true;
    }
}

// File I/O

fn rows_to_string(rows: &[Row]) -> String {
    rows.iter().fold(String::new(), |mut output, r| {
        let _ = writeln!(output, "{}", r.content);
        output
    })
}

fn open(config: &mut EditorConfig, filename: String) {
    config.filename = Some(filename.to_string());
    select_syntax_highlight(config);
    let reader = BufReader::new(File::open(filename).unwrap_or_else(|err| die(err.into())));
    for line in reader.lines() {
        let line = line.unwrap_or_else(|err| die(err.into()));
        insert_row(config, config.row.len(), &line);
    }
    config.dirty = false;
}

fn save(config: &mut EditorConfig) -> Result<()> {
    if config.filename.is_none() {
        let f = prompt(config, "Save as (ESC to cancel):", None)?;
        match f {
            None => {
                set_status_msg(config, "Save aborted".to_string())?;
                return Ok(());
            }
            Some(name) => config.filename = Some(name),
        }
    }

    select_syntax_highlight(config);
    let buf = rows_to_string(&config.row);
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(config.filename.as_ref().unwrap())
        .unwrap_or_else(|err| die(err.into()));
    match file.write(buf.as_bytes()) {
        Ok(bytes) => {
            set_status_msg(config, format!("{} bytes writen to disk", bytes))?;
            config.dirty = false;
        }
        Err(e) => set_status_msg(config, format!("Can't save! I/O error: {}", e))?,
    };
    Ok(())
}

// Find

fn find_callback(config: &mut EditorConfig, query: &str, code: KeyCode) {
    static mut LAST_MATCH: isize = -1;
    static mut DIRECTION: i8 = 1;

    if code == KeyCode::Enter {
        unsafe {
            LAST_MATCH = -1;
            DIRECTION = 1;
        }
        return;
    } else if code == KeyCode::Right || code == KeyCode::Down {
        unsafe {
            DIRECTION = 1;
        }
    } else if code == KeyCode::Left || code == KeyCode::Up {
        unsafe {
            DIRECTION = -1;
        }
    } else {
        unsafe {
            LAST_MATCH = -1;
            DIRECTION = 1;
        }
    }

    let mut current = unsafe {
        if LAST_MATCH == -1 {
            DIRECTION = 1;
        }
        LAST_MATCH
    };

    let row_len = config.row.len();
    for _ in 0..row_len {
        current += unsafe { DIRECTION as isize };
        if current == -1 {
            current = (row_len - 1) as isize;
        } else if current == row_len as isize {
            current = 0;
        }

        let row = &mut config.row[current as usize];
        if let Some(pos) = row.render.find(&query) {
            unsafe { LAST_MATCH = current }
            config.cy = current as usize;
            config.cx = row_rx_to_cx(row, pos);
            config.row_off = row_len;
            break;
        }
    }
}

fn find(config: &mut EditorConfig) -> Result<()> {
    let saved_cx = config.cx;
    let saved_cy = config.cy;
    let saved_col_off = config.col_off;
    let saved_row_off = config.row_off;

    let query = prompt(
        config,
        "Search (Use ESC/Arrows/Enter):",
        Some(Box::new(find_callback)),
    )?;
    if query.is_none() {
        config.cx = saved_cx;
        config.cy = saved_cy;
        config.col_off = saved_col_off;
        config.row_off = saved_row_off;
    }
    Ok(())
}

// Output

fn scroll(config: &mut EditorConfig) {
    config.rx = if config.cy < config.row.len() {
        let row = &config.row[config.cy];
        row_cx_to_rx(row, config.cx)
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

fn draw_rows(config: &mut EditorConfig, buf: &mut String) -> Result<()> {
    for y in 0..config.screen_rows {
        let file_row = y + config.row_off;
        if file_row >= config.row.len() {
            if config.row.is_empty() && y == config.screen_rows / 3 {
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
            let s = config.row[file_row].render[config.col_off..end].to_string();
            let hl = &mut config.row[file_row].hl[config.col_off..end];
            let mut current_color: u8 = 0;

            for (j, ch) in s.chars().into_iter().enumerate() {
                if hl[j] == Highlight::Normal {
                    if current_color != 0 {
                        buf.push_str("\x1b[39m");
                        current_color = 0;
                    }
                    buf.push(ch);
                } else {
                    let color = hl[j].to_color();
                    if color != current_color {
                        current_color = color;
                        let tmp = format!("\x1b[{}m", color);
                        buf.push_str(&tmp);
                    }
                    buf.push(ch);
                }
            }
            buf.push_str("\x1b[39m");
        }

        buf.push_str("\r\n");
    }
    Ok(())
}

fn draw_statusbar(config: &EditorConfig, buf: &mut String) {
    buf.push_str("\x1b[7m");
    let mut status = format!(
        "{} - {} lines {}",
        if let Some(file) = &config.filename {
            file.as_str()
        } else {
            "[No Name]"
        },
        config.row.len(),
        if config.dirty { "(modified)" } else { "" }
    );
    let rstatus = format!(
        "{} | {}/{}",
        if let Some(syntax) = &config.syntax {
            syntax.filetype
        } else {
            "no ft"
        },
        config.cy + 1,
        config.row.len()
    );
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

fn draw_messagebar(config: &mut EditorConfig, buf: &mut String) -> Result<()> {
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

fn refresh_screen(config: &mut EditorConfig) -> Result<()> {
    scroll(config);

    let mut buf = String::new();

    config.stdout.queue(cursor::Hide)?;
    config.stdout.queue(Clear(ClearType::All))?;
    config.stdout.queue(cursor::MoveTo(0, 0))?;

    draw_rows(config, &mut buf)?;
    draw_statusbar(config, &mut buf);
    draw_messagebar(config, &mut buf)?;

    config.stdout.queue(style::Print(buf))?;
    config.stdout.queue(cursor::MoveTo(
        (config.rx - config.col_off) as u16,
        (config.cy - config.row_off) as u16,
    ))?;
    config.stdout.queue(cursor::Show)?;
    config.stdout.flush()?;
    Ok(())
}

fn set_status_msg(config: &mut EditorConfig, msg: String) -> Result<()> {
    config.status_msg = msg;
    config.status_msg_time = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    Ok(())
}

// Input

fn prompt(
    config: &mut EditorConfig,
    p: &str,
    callback: Option<Callback>,
) -> Result<Option<String>> {
    let mut buf = String::new();

    loop {
        set_status_msg(config, format!("{} {}", p, buf))?;
        refresh_screen(config)?;
        let event = read()?;
        if let Event::Key(key) = event {
            match key.code {
                KeyCode::Backspace => {
                    buf.pop();
                }
                KeyCode::Esc => {
                    set_status_msg(config, String::new())?;
                    if let Some(callback) = callback.as_ref() {
                        callback(config, &buf, key.code);
                    }
                    return Ok(None);
                }
                KeyCode::Enter => {
                    if !buf.is_empty() {
                        set_status_msg(config, String::new())?;
                        if let Some(callback) = callback.as_ref() {
                            callback(config, &buf, key.code);
                        }
                        return Ok(Some(buf));
                    }
                }
                KeyCode::Char(c) if !c.is_control() => buf.push(c),
                _ => {}
            }
        }
        if let Some(callback) = callback.as_ref() {
            if let Event::Key(key) = event {
                callback(config, &buf, key.code);
            }
        }
    }
}

fn move_cursor(config: &mut EditorConfig, key: KeyCode) {
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
            if config.row.len() > config.cy {
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

fn process_keypress(config: &mut EditorConfig) -> Result<()> {
    static mut QUIT_TIMES: u8 = KILO_RS_QUIT_TIMES;
    let event = read()?;
    if let Event::Key(key) = event {
        match key.code {
            KeyCode::Right | KeyCode::Left | KeyCode::Up | KeyCode::Down => {
                move_cursor(config, key.code)
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
                    move_cursor(
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
            KeyCode::Enter => insert_newline(config),
            KeyCode::Home => config.cx = 0,
            KeyCode::End if config.cy < config.row.len() => {
                config.cx = config.row[config.cy].content.len()
            }
            KeyCode::Backspace => del_char(config),
            KeyCode::Char('q') if key.modifiers == KeyModifiers::CONTROL => {
                let q = unsafe { QUIT_TIMES };
                if config.dirty && q > 0 {
                    set_status_msg(
                        config,
                        format!(
                            "WARNING!! File has unsaved changes. \
                    Press Ctrl-Q {} more times to quit.",
                            q
                        ),
                    )?;
                    unsafe {
                        QUIT_TIMES -= 1;
                    }
                    return Ok(());
                }
                disable_raw_mode().unwrap();
                execute!(
                    config.stdout,
                    LeaveAlternateScreen,
                    cursor::SetCursorStyle::DefaultUserShape
                )
                .unwrap();
                std::process::exit(0);
            }
            KeyCode::Char('s') if key.modifiers == KeyModifiers::CONTROL => save(config)?,
            KeyCode::Char('f') if key.modifiers == KeyModifiers::CONTROL => find(config)?,
            KeyCode::Char(c) => insert_char(config, c),
            _ => {}
        }
    }
    unsafe {
        QUIT_TIMES = KILO_RS_QUIT_TIMES;
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
        open(&mut config, filename);
    }
    set_status_msg(
        &mut config,
        "HELP: Ctrl-S = save | Ctrl-Q = quit | Ctrl-F = find".to_string(),
    )
    .unwrap_or_else(|err| die(err));
    loop {
        refresh_screen(&mut config).unwrap_or_else(|err| die(err));
        process_keypress(&mut config).unwrap_or_else(|err| die(err));
    }
}
