use crossterm::{
    cursor::{self, MoveTo},
    event::{self, Event, KeyCode, KeyEventKind},
    execute, queue,
    style::{Attribute, Color, Print, ResetColor, SetAttribute, SetForegroundColor},
    terminal::{self, Clear, ClearType, disable_raw_mode, enable_raw_mode},
};
use serde_json::Value;
use std::collections::HashSet;
use std::env;
use std::fs::File;
use std::io::{self, Read, Write};
use std::process::{Command, Stdio};

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum PathSeg {
    String(String),
    Index(usize),
}

#[derive(Clone)]
struct Row {
    path: Vec<PathSeg>,
    text: String,
    selectable: bool,
    is_container: bool,
}

fn path_to_jq(path: &[PathSeg]) -> String {
    let mut out = String::from(".");
    for seg in path {
        match seg {
            PathSeg::Index(i) => out.push_str(&format!("[{}]", i)),
            PathSeg::String(s) => {
                if is_identifier(s) {
                    if out == "." {
                        out.push_str(s);
                    } else {
                        out.push_str(&format!(".{}", s));
                    }
                } else {
                    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
                    out.push_str(&format!("[\"{}\"]", escaped));
                }
            }
        }
    }
    out
}

fn path_to_jq_all_arrays(path: &[PathSeg]) -> String {
    let mut out = String::from(".");
    for seg in path {
        match seg {
            PathSeg::Index(_) => out.push_str("[]"),
            PathSeg::String(s) => {
                if is_identifier(s) {
                    if out == "." {
                        out.push_str(s);
                    } else {
                        out.push_str(&format!(".{}", s));
                    }
                } else {
                    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
                    out.push_str(&format!("[\"{}\"]", escaped));
                }
            }
        }
    }
    out
}

fn is_identifier(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let mut chars = s.chars();
    let first = chars.next().unwrap();
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn build_rows(value: &Value, expanded: &HashSet<Vec<PathSeg>>) -> Vec<Row> {
    let mut rows = Vec::new();

    fn render(
        node: &Value,
        path: Vec<PathSeg>,
        depth: usize,
        is_last: bool,
        key: Option<&str>,
        rows: &mut Vec<Row>,
        expanded: &HashSet<Vec<PathSeg>>,
    ) {
        let indent = "  ".repeat(depth);
        let key_prefix = if let Some(k) = key {
            format!("{}: ", serde_json::to_string(k).unwrap())
        } else {
            String::new()
        };
        let comma = if is_last { "" } else { "," };

        match node {
            Value::Object(map) => {
                if expanded.contains(&path) || key.is_none() {
                    rows.push(Row {
                        path: path.clone(),
                        text: format!("{}{}{{", indent, key_prefix),
                        selectable: key.is_some(),
                        is_container: true,
                    });
                    let items: Vec<_> = map.iter().collect();
                    for (i, (k, v)) in items.iter().enumerate() {
                        let mut cp = path.clone();
                        cp.push(PathSeg::String((*k).clone()));
                        render(v, cp, depth + 1, i == items.len() - 1, Some(k), rows, expanded);
                    }
                    rows.push(Row {
                        path: path.clone(),
                        text: format!("{}}}{}", indent, comma),
                        selectable: false,
                        is_container: false,
                    });
                } else {
                    rows.push(Row {
                        path: path.clone(),
                        text: format!("{}{}{{...}}{}", indent, key_prefix, comma),
                        selectable: true,
                        is_container: true,
                    });
                }
            }
            Value::Array(arr) => {
                if expanded.contains(&path) || key.is_none() {
                    rows.push(Row {
                        path: path.clone(),
                        text: format!("{}{}[", indent, key_prefix),
                        selectable: key.is_some(),
                        is_container: true,
                    });
                    for (i, v) in arr.iter().enumerate() {
                        let mut cp = path.clone();
                        cp.push(PathSeg::Index(i));
                        render(v, cp, depth + 1, i == arr.len() - 1, None, rows, expanded);
                    }
                    rows.push(Row {
                        path: path.clone(),
                        text: format!("{}]{}", indent, comma),
                        selectable: false,
                        is_container: false,
                    });
                } else {
                    rows.push(Row {
                        path: path.clone(),
                        text: format!("{}{}[...]{}", indent, key_prefix, comma),
                        selectable: true,
                        is_container: true,
                    });
                }
            }
            _ => {
                let value_text = serde_json::to_string(node).unwrap();
                rows.push(Row {
                    path: path.clone(),
                    text: format!("{}{}{}{}", indent, key_prefix, value_text, comma),
                    selectable: true,
                    is_container: false,
                });
            }
        }
    }

    render(value, vec![], 0, true, None, &mut rows, expanded);
    rows
}

fn expand_all(v: &Value, p: Vec<PathSeg>, expanded: &mut HashSet<Vec<PathSeg>>) {
    match v {
        Value::Object(map) => {
            for (k, child) in map {
                let mut cp = p.clone();
                cp.push(PathSeg::String(k.clone()));
                if child.is_object() || child.is_array() {
                    expanded.insert(cp.clone());
                    expand_all(child, cp, expanded);
                }
            }
        }
        Value::Array(arr) => {
            for (i, child) in arr.iter().enumerate() {
                let mut cp = p.clone();
                cp.push(PathSeg::Index(i));
                if child.is_object() || child.is_array() {
                    expanded.insert(cp.clone());
                    expand_all(child, cp, expanded);
                }
            }
        }
        _ => {}
    }
}

fn shell_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\"'\"'"))
}

fn generate_jq_command(picked: &[Vec<PathSeg>], in_path: &str) -> (String, String) {
    if picked.is_empty() {
        return (String::new(), String::new());
    }

    let mut unique_paths = Vec::new();
    for p in picked {
        if !unique_paths.contains(p) {
            unique_paths.push(p.clone());
        }
    }

    let mut normalized = Vec::new();
    for p in unique_paths {
        let njq = path_to_jq_all_arrays(&p);
        if !normalized.contains(&njq) {
            normalized.push(njq);
        }
    }

    let mut entries = Vec::new();
    for p in normalized {
        let key = serde_json::to_string(&p).unwrap();
        entries.push(format!("{}: {}", key, p));
    }

    let jq_filter = format!("{{ {} }}", entries.join(", "));
    
    let cmd = if !in_path.is_empty() {
        format!("jq {} {}", shell_single_quote(&jq_filter), shell_single_quote(in_path))
    } else {
        format!("jq {}", shell_single_quote(&jq_filter))
    };
    
    (jq_filter, cmd)
}

fn get_preview(jq_filter: &str, in_path: &str, text: &str) -> String {
    if jq_filter.is_empty() {
        return String::new();
    }
    
    let mut child_cmd = Command::new("jq");
    child_cmd.arg(jq_filter);
    if !in_path.is_empty() {
        child_cmd.arg(in_path);
        child_cmd.stdin(Stdio::null());
    } else {
        child_cmd.stdin(Stdio::piped());
    }
    child_cmd.stdout(Stdio::piped());
    child_cmd.stderr(Stdio::piped());

    if let Ok(mut child) = child_cmd.spawn() {
        if in_path.is_empty() {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
        }
        if let Ok(output) = child.wait_with_output() {
            if output.status.success() {
                return String::from_utf8_lossy(&output.stdout).into_owned();
            } else {
                return String::from_utf8_lossy(&output.stderr).into_owned();
            }
        }
    }
    "Failed to run jq".to_string()
}

fn main() -> io::Result<()> {
    let args: Vec<String> = env::args().collect();
    let mut in_path = String::new();
    let mut text = String::new();

    if args.len() > 1 && args[1] != "-" {
        in_path = args[1].clone();
        let mut f = File::open(&in_path).unwrap_or_else(|_| {
            eprintln!("Usage: jw [file.json]   or   cat data.json | jw");
            std::process::exit(1);
        });
        f.read_to_string(&mut text)?;
    } else if !unsafe { libc::isatty(libc::STDIN_FILENO) == 1 } {
        io::stdin().read_to_string(&mut text)?;
    } else {
        eprintln!("Usage: jw [file.json]   or   cat data.json | jw");
        std::process::exit(1);
    }

    let data: Value = serde_json::from_str(&text).unwrap_or_else(|_| {
        eprintln!("error: invalid JSON");
        std::process::exit(1);
    });

    if !unsafe { libc::isatty(libc::STDIN_FILENO) == 1 } {
        // Try to reopen terminal for input since stdin was piped.
        // On macOS, opening "/dev/tty" directly fails with kqueue (used by crossterm).
        // Instead, we get the real tty device from stdout.
        unsafe {
            let mut tty_fd = -1;
            let tty_name = libc::ttyname(libc::STDOUT_FILENO);
            if !tty_name.is_null() {
                tty_fd = libc::open(tty_name, libc::O_RDWR);
            }
            if tty_fd < 0 {
                tty_fd = libc::open(b"/dev/tty\0".as_ptr() as *const i8, libc::O_RDWR);
            }
            if tty_fd < 0 {
                eprintln!("error: interactive TUI requires a terminal (stdin is piped and /dev/tty is unavailable)");
                std::process::exit(1);
            }
            libc::dup2(tty_fd, libc::STDIN_FILENO);
            libc::close(tty_fd);
        }
    }
    
    if unsafe { libc::isatty(libc::STDOUT_FILENO) == 0 } {
        eprintln!("error: interactive TUI requires a terminal stdout");
        std::process::exit(1);
    }

    let mut expanded = HashSet::new();
    expand_all(&data, vec![], &mut expanded);

    let picked = run_tui(&data, &mut expanded, &in_path, &text)?;
    if picked.is_empty() {
        return Ok(());
    }

    let (_, final_cmd) = generate_jq_command(&picked, &in_path);

    println!("{}", final_cmd);

    // Also send to pbcopy
    if let Ok(mut child) = Command::new("pbcopy").stdin(Stdio::piped()).spawn() {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(final_cmd.as_bytes());
        }
        let _ = child.wait();
    }

    Ok(())
}

fn range_indices(a: usize, b: usize) -> std::ops::RangeInclusive<usize> {
    if a <= b {
        a..=b
    } else {
        b..=a
    }
}

fn run_tui(data: &Value, expanded: &mut HashSet<Vec<PathSeg>>, in_path: &str, text: &str) -> io::Result<Vec<Vec<PathSeg>>> {
    let mut stdout = io::stdout();
    enable_raw_mode()?;
    execute!(stdout, terminal::EnterAlternateScreen, cursor::Hide)?;

    let mut selected = HashSet::new();
    let mut cursor = 0;
    let mut top = 0;
    let mut visual = false;
    let mut visual_anchor: Option<usize> = None;
    let mut search_term = String::new();
    let mut last_search: Option<usize> = None;
    let mut rows = build_rows(data, expanded);
    let mut need_rebuild = false;

    let mut result = Vec::new();

    let mut last_preview_paths: Vec<Vec<PathSeg>> = Vec::new();
    let mut preview_jq_cmd = String::new();
    let mut preview_lines: Vec<String> = Vec::new();

    loop {
        if need_rebuild {
            rows = build_rows(data, expanded);
            need_rebuild = false;
        }
        if rows.is_empty() {
            break;
        }
        cursor = cursor.min(rows.len().saturating_sub(1));
        
        let (w_u16, h_u16) = terminal::size()?;
        let w = w_u16 as usize;
        let h = h_u16 as usize;
        let body_h = 1.max(h.saturating_sub(2));

        let left_w = w / 2;
        let right_start_x = left_w + 1;
        let right_w = w.saturating_sub(right_start_x);

        let wrapped_cmd = wrap_text(&preview_jq_cmd, right_w);
        let cmd_lines_count = wrapped_cmd.len();

        if cursor < top {
            top = cursor;
        } else if cursor >= top + body_h {
            top = cursor - body_h + 1;
        }

        // --- Preview Evaluation ---
        let current_target = if selected.is_empty() {
            vec![rows[cursor].path.clone()]
        } else {
            selected.iter().cloned().collect()
        };

        let mut current_target_sorted = current_target.clone();
        current_target_sorted.sort();

        if current_target_sorted != last_preview_paths {
            last_preview_paths = current_target_sorted.clone();
            let (filter, cmd) = generate_jq_command(&current_target_sorted, in_path);
            preview_jq_cmd = cmd;
            let preview_output = get_preview(&filter, in_path, text);
            preview_lines = preview_output.lines().map(|s| s.to_string()).collect();
        }
        // --------------------------

        queue!(stdout, Clear(ClearType::All))?;

        for i in 0..body_h {
            // Draw Separator
            queue!(stdout, MoveTo(left_w as u16, i as u16), SetForegroundColor(Color::DarkGrey), Print("│"), ResetColor)?;

            // Draw Left Panel
            let idx = top + i;
            if idx < rows.len() {
                let row = &rows[idx];
                let left_bar = if row.selectable && selected.contains(&row.path) { "▌" } else { " " };
                let line = &row.text;
                
                let mut is_visual = false;
                if visual {
                    if let Some(anchor) = visual_anchor {
                        if range_indices(anchor, cursor).contains(&idx) {
                            is_visual = true;
                        }
                    }
                }

                let is_cursor = idx == cursor;
                let max_line_len = left_w.saturating_sub(3);
                
                queue!(stdout, MoveTo(0, i as u16))?;
                
                if is_cursor {
                    queue!(
                        stdout,
                        SetAttribute(Attribute::Reverse),
                        SetForegroundColor(Color::Cyan),
                        Print(format!("{} {}", left_bar, truncate(line, max_line_len))),
                        ResetColor,
                        SetAttribute(Attribute::Reset)
                    )?;
                } else {
                    if is_visual {
                        queue!(stdout, SetAttribute(Attribute::Reverse), SetForegroundColor(Color::Yellow))?;
                    }
                    
                    if row.selectable && selected.contains(&row.path) {
                        queue!(stdout, SetForegroundColor(Color::Green), SetAttribute(Attribute::Bold), Print(left_bar), ResetColor)?;
                    } else {
                        queue!(stdout, Print(left_bar))?;
                    }
                    
                    if is_visual {
                        queue!(stdout, SetAttribute(Attribute::Reverse), SetForegroundColor(Color::Yellow))?;
                    }
                    queue!(stdout, Print(" "))?;

                    queue!(stdout, Print(truncate(line, max_line_len)))?;
                    
                    if is_visual {
                        queue!(stdout, ResetColor, SetAttribute(Attribute::Reset))?;
                    }
                }
            }

            // Draw Right Panel (Preview)
            if right_w > 0 {
                let rx = right_start_x as u16;
                let ry = i as u16;
                queue!(stdout, MoveTo(rx, ry))?;
                
                if i == 0 {
                    queue!(stdout, SetForegroundColor(Color::Yellow), Print(truncate("Preview:", right_w)), ResetColor)?;
                } else if i >= 1 && i <= cmd_lines_count {
                    queue!(stdout, SetAttribute(Attribute::Bold), Print(&wrapped_cmd[i - 1]), SetAttribute(Attribute::Reset))?;
                } else if i == cmd_lines_count + 1 {
                    queue!(stdout, SetForegroundColor(Color::DarkGrey), Print("─".repeat(right_w)), ResetColor)?;
                } else {
                    let out_idx = i.saturating_sub(cmd_lines_count + 2);
                    if i > cmd_lines_count + 1 && out_idx < preview_lines.len() {
                        queue!(stdout, Print(truncate(&preview_lines[out_idx], right_w)))?;
                    }
                }
            }
        }

        queue!(stdout, MoveTo(0, (h.saturating_sub(2)) as u16))?;
        let mode = if visual { "VISUAL" } else { "NORMAL" };
        let footer = format!("{}  /:search n:next  Space:toggle  Tab:toggle+down  v:visual  .:expand/collapse  Enter:extract  q:quit", mode);
        queue!(stdout, SetForegroundColor(Color::Green), Print(truncate(&footer, w)), ResetColor)?;
        
        queue!(stdout, MoveTo(0, (h.saturating_sub(1)) as u16))?;
        let jq_preview = path_to_jq(&rows[cursor].path);
        queue!(stdout, SetAttribute(Attribute::Dim), Print(truncate(&format!("cursor: {}", jq_preview), w)), SetAttribute(Attribute::Reset))?;

        stdout.flush()?;

        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Enter => {
                        result = selected.into_iter().collect();
                        // If nothing is selected, use the current cursor as the single selected item
                        if result.is_empty() {
                            if rows[cursor].selectable {
                                result.push(rows[cursor].path.clone());
                            } else {
                                // If not selectable, try to extract anyway (it's a container)
                                result.push(rows[cursor].path.clone());
                            }
                        }
                        break;
                    }
                    KeyCode::Char('j') | KeyCode::Down => cursor = cursor.saturating_add(1).min(rows.len().saturating_sub(1)),
                    KeyCode::Char('k') | KeyCode::Up => cursor = cursor.saturating_sub(1),
                    KeyCode::Char('g') => cursor = 0,
                    KeyCode::Char('G') => cursor = rows.len().saturating_sub(1),
                    KeyCode::Char('h') | KeyCode::Left => {
                        let row = &rows[cursor];
                        if expanded.contains(&row.path) {
                            expanded.remove(&row.path);
                            need_rebuild = true;
                        } else if !row.path.is_empty() {
                            let mut parent = row.path.clone();
                            parent.pop();
                            for (i, r) in rows.iter().enumerate() {
                                if r.path == parent {
                                    cursor = i;
                                    break;
                                }
                            }
                        }
                    }
                    KeyCode::Char('l') | KeyCode::Right => {
                        let row = &rows[cursor];
                        if row.is_container {
                            expanded.insert(row.path.clone());
                            need_rebuild = true;
                        }
                    }
                    KeyCode::Char('.') => {
                        let row = &rows[cursor];
                        if row.is_container {
                            if expanded.contains(&row.path) {
                                expanded.remove(&row.path);
                            } else {
                                expanded.insert(row.path.clone());
                            }
                            need_rebuild = true;
                        }
                    }
                    KeyCode::Char('v') => {
                        if visual {
                            visual = false;
                            visual_anchor = None;
                        } else {
                            visual = true;
                            visual_anchor = Some(cursor);
                        }
                    }
                    KeyCode::Char(' ') => {
                        let target = if visual && visual_anchor.is_some() {
                            range_indices(visual_anchor.unwrap(), cursor).collect::<Vec<_>>()
                        } else {
                            vec![cursor]
                        };
                        let first_sel = rows[target[0]].selectable && selected.contains(&rows[target[0]].path);
                        for idx in target {
                            if !rows[idx].selectable { continue; }
                            let p = &rows[idx].path;
                            if first_sel {
                                selected.remove(p);
                            } else {
                                selected.insert(p.clone());
                            }
                        }
                    }
                    KeyCode::Tab => {
                        let target = if visual && visual_anchor.is_some() {
                            range_indices(visual_anchor.unwrap(), cursor).collect::<Vec<_>>()
                        } else {
                            vec![cursor]
                        };
                        for idx in target {
                            if rows[idx].selectable {
                                selected.insert(rows[idx].path.clone());
                            }
                        }
                        cursor = cursor.saturating_add(1).min(rows.len().saturating_sub(1));
                    }
                    KeyCode::Char('/') => {
                        queue!(stdout, MoveTo(0, (h.saturating_sub(1)) as u16), Clear(ClearType::CurrentLine), Print("/"))?;
                        queue!(stdout, cursor::Show)?;
                        stdout.flush()?;
                        
                        let mut q = String::new();
                        loop {
                            if let Event::Key(k) = event::read()? {
                                if k.kind == KeyEventKind::Press {
                                    match k.code {
                                        KeyCode::Enter => break,
                                        KeyCode::Esc => {
                                            q.clear();
                                            break;
                                        }
                                        KeyCode::Backspace => {
                                            q.pop();
                                            queue!(stdout, MoveTo(0, (h.saturating_sub(1)) as u16), Clear(ClearType::CurrentLine), Print(format!("/{}", q)))?;
                                            stdout.flush()?;
                                        }
                                        KeyCode::Char(c) => {
                                            q.push(c);
                                            queue!(stdout, Print(c))?;
                                            stdout.flush()?;
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                        queue!(stdout, cursor::Hide)?;
                        search_term = q.trim().to_lowercase();
                        if !search_term.is_empty() {
                            for i in (cursor + 1)..rows.len() {
                                if rows[i].text.to_lowercase().contains(&search_term) {
                                    cursor = i;
                                    last_search = Some(i);
                                    break;
                                }
                            }
                        }
                    }
                    KeyCode::Char('n') => {
                        if !search_term.is_empty() {
                            let start = if let Some(ls) = last_search { ls + 1 } else { cursor + 1 };
                            let mut found = None;
                            for i in start..rows.len() {
                                if rows[i].text.to_lowercase().contains(&search_term) {
                                    found = Some(i);
                                    break;
                                }
                            }
                            if found.is_none() {
                                for i in 0..start {
                                    if rows[i].text.to_lowercase().contains(&search_term) {
                                        found = Some(i);
                                        break;
                                    }
                                }
                            }
                            if let Some(f) = found {
                                cursor = f;
                                last_search = Some(f);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    execute!(stdout, terminal::LeaveAlternateScreen, cursor::Show)?;
    disable_raw_mode()?;

    Ok(result)
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.chars().count() > max_len {
        s.chars().take(max_len).collect()
    } else {
        s.to_string()
    }
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let mut lines = Vec::new();
    let mut current_line = String::new();
    for c in text.chars() {
        if current_line.chars().count() == width {
            lines.push(current_line);
            current_line = String::new();
        }
        current_line.push(c);
    }
    if !current_line.is_empty() {
        lines.push(current_line);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}
