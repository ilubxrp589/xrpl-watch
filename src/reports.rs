//! The DAILY REPORTS page (key `l`, for log): the daily engineering reports, newest first. ↑ ↓ (or j k) pick a day
//! and Enter (or →) opens it; in a report ↑ ↓ PgUp PgDn Space scroll, Home / End jump, Esc (or ← or Backspace) goes
//! back to the list. The reports are the public ilubxrp589/xrpl-validator-reports, written on .39 each night at 00:20
//! (~/xrpl-ops/daily_report); the same job copies their .md files to /mnt/xrpl-data/daily-reports/ here (env
//! DAILY_REPORTS overrides). View only.

use super::*;
use std::path::PathBuf;

fn dir() -> PathBuf {
    PathBuf::from(std::env::var("DAILY_REPORTS").ok().filter(|v| !v.is_empty()).unwrap_or_else(|| "/mnt/xrpl-data/daily-reports".to_string()))
}

struct Entry {
    date: String,
    path: PathBuf,
    /// "13 commits · 913/913 ledgers matched", from the report's Summary.
    summary: String,
    /// Compiled afterwards from the git history (no live snapshot).
    retro: bool,
}

/// Where the reader is: the day picked, whether it is open, and how far down it is scrolled.
struct View {
    sel: usize,
    open: bool,
    scroll: usize,
    /// The list's first row on screen.
    top: usize,
    /// The reader's rows and the open report's length, from the last frame (for PgUp / PgDn / End).
    page_rows: usize,
    doc_len: usize,
    /// --report DATE: open that day once the list is read.
    want: Option<String>,
}

static VIEW: Mutex<View> = Mutex::new(View { sel: 0, open: false, scroll: 0, top: 0, page_rows: 20, doc_len: 0, want: None });

struct Cache {
    scanned: Option<Instant>,
    list: Vec<Entry>,
    /// The report on screen, laid out for a width: (date, width, lines).
    doc: Option<(String, usize, Vec<Line<'static>>)>,
}

static CACHE: Mutex<Cache> = Mutex::new(Cache { scanned: None, list: Vec::new(), doc: None });

/// --once / --record: open the report of `date` (YYYY-MM-DD) scrolled down `scroll` lines.
pub fn preset(date: Option<String>, scroll: usize) {
    let mut v = VIEW.lock().unwrap_or_else(|e| e.into_inner());
    v.want = date;
    v.scroll = scroll;
}

/// Read the list again (each minute: a new report lands once a night).
fn scan(c: &mut Cache) {
    if c.scanned.is_some_and(|t| t.elapsed() < Duration::from_secs(60)) {
        return;
    }
    c.scanned = Some(Instant::now());
    let mut list = Vec::new();
    if let Ok(rd) = fs::read_dir(dir()) {
        for e in rd.flatten() {
            let path = e.path();
            let Some(date) = path.file_name().and_then(|n| n.to_str()).and_then(|n| n.strip_suffix(".md")).map(str::to_string) else {
                continue;
            };
            let b = date.as_bytes();
            if b.len() != 10 || b[4] != b'-' || b[7] != b'-' || !date.chars().filter(|c| *c != '-').all(|c| c.is_ascii_digit()) {
                continue;
            }
            let text = fs::read_to_string(&path).unwrap_or_default();
            let (summary, retro) = summarize(&text);
            list.push(Entry { date, path, summary, retro });
        }
    }
    list.sort_by(|a, b| b.date.cmp(&a.date));
    c.list = list;
    c.doc = None; // a report rewritten since is laid out again
}

/// The list line for a report: its commit count, findings fixed and the live shadow's ledgers, from its Summary.
fn summarize(text: &str) -> (String, bool) {
    let retro = text.contains("Compiled retrospectively");
    let mut parts: Vec<String> = Vec::new();
    let mut in_sum = false;
    for l in text.lines() {
        if let Some(h) = l.strip_prefix("## ") {
            if in_sum {
                break;
            }
            in_sum = h == "Summary";
            continue;
        }
        let Some(b) = l.strip_prefix("- ").filter(|_| in_sum) else { continue };
        let b = b.replace("**", "");
        if parts.is_empty() && b.contains(" commit") {
            parts.push(b.split(" (").next().unwrap_or(&b).to_string());
        } else if b.contains("findings fixed") || b.contains("finding fixed") {
            let k = b.split_whitespace().next().unwrap_or("?");
            parts.push(format!("{k} fixed"));
        } else if let Some(rest) = b.strip_prefix("Live shadow: ") {
            if let Some(tok) = rest.split_whitespace().find(|t| t.contains('/')) {
                parts.push(format!("{tok} ledgers"));
            }
        }
    }
    (parts.join(" · "), retro)
}

/// "Wed" for "2026-10-14" (days from 1970-01-01, a Thursday).
fn weekday(date: &str) -> &'static str {
    let n = |a: usize, b: usize| date.get(a..b).and_then(|s| s.parse::<i64>().ok()).unwrap_or(1);
    let (y, m, d) = (n(0, 4), n(5, 7), n(8, 10));
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + d - 1;
    let days = era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468;
    ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"][days.rem_euclid(7) as usize]
}

// ---- the reports' markdown, laid out for the terminal ----

type Piece = (String, Style);

/// **bold**, `code` and [text](link) inside a line (underscores stay: identifiers like fixCleanup3_4_0 carry them).
fn inline(s: &str, base: Style) -> Vec<Piece> {
    let p = pal();
    let (code, link) = (Style::new().fg(p.feed), Style::new().fg(p.violet).add_modifier(Modifier::UNDERLINED));
    let cs: Vec<char> = s.chars().collect();
    let mut out: Vec<Piece> = Vec::new();
    let mut buf = String::new();
    let mut strong = false;
    let style = |strong: bool| if strong { base.add_modifier(Modifier::BOLD) } else { base };
    let mut i = 0;
    while i < cs.len() {
        if cs[i] == '*' && cs.get(i + 1) == Some(&'*') {
            if !buf.is_empty() {
                out.push((std::mem::take(&mut buf), style(strong)));
            }
            strong = !strong;
            i += 2;
            continue;
        }
        if cs[i] == '`' {
            if let Some(j) = cs[i + 1..].iter().position(|&c| c == '`') {
                if !buf.is_empty() {
                    out.push((std::mem::take(&mut buf), style(strong)));
                }
                out.push((cs[i + 1..i + 1 + j].iter().collect(), code));
                i += j + 2;
                continue;
            }
        }
        if cs[i] == '[' {
            if let Some(j) = cs[i + 1..].iter().position(|&c| c == ']') {
                let k = i + 1 + j;
                if cs.get(k + 1) == Some(&'(') {
                    if let Some(m) = cs[k + 2..].iter().position(|&c| c == ')') {
                        if !buf.is_empty() {
                            out.push((std::mem::take(&mut buf), style(strong)));
                        }
                        out.push((cs[i + 1..k].iter().collect(), link));
                        i = k + 3 + m;
                        continue;
                    }
                }
            }
        }
        buf.push(cs[i]);
        i += 1;
    }
    if !buf.is_empty() {
        out.push((buf, style(strong)));
    }
    out
}

fn width_of(s: &str) -> usize {
    s.chars().count()
}

/// Word-wrap `pieces` to `width`: the first line after `first` (already laid spans), the rest indented `indent`.
fn wrap(pieces: Vec<Piece>, width: usize, first: Vec<Span<'static>>, indent: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut cur: Vec<Span<'static>> = first;
    let mut used: usize = cur.iter().map(|s| width_of(&s.content)).sum();
    let mut fresh = true; // nothing but the lead-in on this line yet
    let mut space = false; // a space came before the next word (it may end the previous piece)
    for (text, st) in pieces {
        for (k, w) in text.split(' ').enumerate() {
            if k > 0 {
                space = true;
            }
            if w.is_empty() {
                continue;
            }
            let mut w = w.to_string();
            let gap = usize::from(space && !fresh);
            if !fresh && used + gap + width_of(&w) > width {
                lines.push(Line::from(std::mem::take(&mut cur)));
                cur.push(Span::raw(" ".repeat(indent)));
                used = indent;
                fresh = true;
            }
            if space && !fresh {
                cur.push(Span::styled(" ", st));
                used += 1;
            }
            space = false;
            // a word longer than the line is cut
            while used + width_of(&w) > width && width > used + 1 {
                let take = width - used;
                let head: String = w.chars().take(take).collect();
                w = w.chars().skip(take).collect();
                cur.push(Span::styled(head, st));
                lines.push(Line::from(std::mem::take(&mut cur)));
                cur.push(Span::raw(" ".repeat(indent)));
                used = indent;
            }
            used += width_of(&w);
            cur.push(Span::styled(w, st));
            fresh = false;
        }
    }
    lines.push(Line::from(cur));
    lines
}

/// One markdown table: every column but the last as wide as its widest cell, the last wraps in what is left.
fn table(rows: &[&str], width: usize, out: &mut Vec<Line<'static>>) {
    let p = pal();
    let cells: Vec<Vec<String>> = rows
        .iter()
        .map(|r| r.trim().trim_matches('|').split('|').map(|c| c.trim().to_string()).collect::<Vec<_>>())
        .filter(|r| !r.iter().all(|c| !c.is_empty() && c.chars().all(|ch| ch == '-' || ch == ':')))
        .collect();
    let Some(ncol) = cells.iter().map(Vec::len).max() else { return };
    let plain = |c: &str| inline(c, Style::new()).into_iter().map(|(t, _)| t).collect::<String>();
    let mut wcol = vec![0usize; ncol];
    for r in &cells {
        for (k, c) in r.iter().enumerate() {
            wcol[k] = wcol[k].max(width_of(&plain(c)).min(40));
        }
    }
    for (ri, r) in cells.iter().enumerate() {
        let base = if ri == 0 { Style::new().fg(p.violet).add_modifier(Modifier::BOLD) } else { Style::new().fg(p.ink) };
        let mut lead: Vec<Span<'static>> = vec![Span::raw("  ")];
        let mut used = 2;
        for (k, &cw) in wcol.iter().enumerate().take(ncol.saturating_sub(1)) {
            let c = r.get(k).map(String::as_str).unwrap_or("");
            let mut w = 0;
            for (t, st) in inline(c, base) {
                let t: String = t.chars().take(cw.saturating_sub(w)).collect();
                w += width_of(&t);
                lead.push(Span::styled(t, st));
            }
            lead.push(Span::raw(" ".repeat(cw - w + 2)));
            used += cw + 2;
        }
        let last = r.get(ncol - 1).map(String::as_str).unwrap_or("");
        out.extend(wrap(inline(last, base), width, lead, used));
    }
}

/// A report's markdown as terminal lines `width` wide.
fn render(text: &str, width: usize) -> Vec<Line<'static>> {
    let p = pal();
    let width = width.max(24);
    let ink = Style::new().fg(p.ink);
    let mut out: Vec<Line<'static>> = Vec::new();
    let src: Vec<&str> = text.lines().collect();
    let mut blank = true;
    let mut i = 0;
    while i < src.len() {
        let l = src[i].trim_end();
        if l.trim().is_empty() {
            if !blank {
                out.push(Line::from(""));
                blank = true;
            }
            i += 1;
            continue;
        }
        blank = false;
        if let Some(h) = l.strip_prefix("# ") {
            out.extend(wrap(inline(h, Style::new().fg(p.gold).add_modifier(Modifier::BOLD)), width, vec![], 0));
            out.push(Line::from(sp("━".repeat(width.min(width_of(h) + 4)), p.gold)));
        } else if let Some(h) = l.strip_prefix("## ") {
            out.extend(wrap(inline(h, Style::new().fg(p.violet).add_modifier(Modifier::BOLD)), width, vec![sp("━━ ", p.violet)], 3));
        } else if let Some(h) = l.strip_prefix("### ") {
            out.extend(wrap(inline(h, Style::new().fg(p.pink).add_modifier(Modifier::BOLD)), width, vec![sp("── ", p.pink)], 3));
        } else if l.starts_with('|') {
            let j = (i..src.len()).find(|&j| !src[j].trim_start().starts_with('|')).unwrap_or(src.len());
            table(&src[i..j], width, &mut out);
            i = j;
            continue;
        } else if let Some(b) = l.strip_prefix("- ") {
            out.extend(wrap(inline(b, ink), width, vec![sp("  • ", p.gold)], 4));
        } else if let Some(b) = l.strip_prefix("  - ") {
            out.extend(wrap(inline(b, ink), width, vec![sp("      ◦ ", p.gold)], 8));
        } else if l.starts_with("  ") {
            out.extend(wrap(inline(l.trim_start(), Style::new().fg(p.dim)), width, vec![Span::raw("    ")], 4));
        } else if l.len() > 2 && l.starts_with('_') && l.ends_with('_') {
            out.extend(wrap(inline(&l[1..l.len() - 1], Style::new().fg(p.dim).add_modifier(Modifier::ITALIC)), width, vec![], 0));
        } else {
            out.extend(wrap(inline(l, ink), width, vec![], 0));
        }
        i += 1;
    }
    while out.last().is_some_and(|l| l.width() == 0) {
        out.pop();
    }
    out
}

// ---- the page ----

pub fn draw(f: &mut Frame, g: &Shared, paused: bool) {
    let p = pal();
    let area = f.area();
    f.render_widget(Block::new().style(Style::new().bg(p.bg)), area);
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Fill(1), Constraint::Length(1)]).split(area);
    draw_header(f, rows[0], &g.snap);
    let cols = Layout::horizontal([Constraint::Length(52), Constraint::Fill(1)]).split(rows[1]);
    let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    scan(&mut c);
    let mut v = VIEW.lock().unwrap_or_else(|e| e.into_inner());
    let n = c.list.len();
    if let Some(want) = v.want.take() {
        if let Some(k) = c.list.iter().position(|e| e.date == want) {
            v.sel = k;
            v.open = true;
        }
    }
    if n == 0 {
        let msg = vec![
            Line::from(""),
            Line::from(dim(format!(" No reports in {} yet.", dir().display()))),
            Line::from(dim(" The nightly report job on .39 copies them here after it writes each day's report (00:20).")),
        ];
        f.render_widget(Paragraph::new(msg).block(panel(vec![bold("DAILY REPORTS", p.gold)], p.gold)), rows[1]);
        draw_footer(f, rows[2], &g.snap, paused);
        return;
    }
    v.sel = v.sel.min(n - 1);

    // The days.
    let list_h = cols[0].height.saturating_sub(2) as usize;
    if v.sel < v.top {
        v.top = v.sel;
    } else if v.sel >= v.top + list_h.max(1) {
        v.top = v.sel + 1 - list_h.max(1);
    }
    let sum_w = (cols[0].width as usize).saturating_sub(19);
    let lines: Vec<Line> = c
        .list
        .iter()
        .enumerate()
        .skip(v.top)
        .take(list_h)
        .map(|(k, e)| {
            let on = k == v.sel;
            let summary: String = if e.retro { format!("{} · retro", e.summary) } else { e.summary.clone() };
            let summary: String = if summary.chars().count() > sum_w {
                summary.chars().take(sum_w.saturating_sub(1)).chain(std::iter::once('…')).collect()
            } else {
                summary
            };
            if on {
                let st = Style::new().fg(p.bg).bg(if v.open { p.violet } else { p.gold }).add_modifier(Modifier::BOLD);
                Line::from(vec![Span::styled(format!("▸ {} {} {:<w$}", e.date, weekday(&e.date), summary, w = sum_w), st)])
            } else {
                Line::from(vec![
                    Span::raw("  "),
                    ink(format!("{} ", e.date)),
                    dim(format!("{} ", weekday(&e.date))),
                    sp(summary, if e.retro { p.dim } else { p.feed }),
                ])
            }
        })
        .collect();
    let lc = if v.open { p.dim } else { p.gold };
    f.render_widget(
        Paragraph::new(lines).block(panel(vec![bold("DAILY REPORTS", p.gold), dim(format!(" · {n} days, newest first"))], lc)),
        cols[0],
    );

    // The report.
    let rw = (cols[1].width as usize).saturating_sub(4);
    let (date, path) = (c.list[v.sel].date.clone(), c.list[v.sel].path.clone());
    if c.doc.as_ref().is_none_or(|(d, w, _)| *d != date || *w != rw) {
        let text = fs::read_to_string(&path).unwrap_or_default();
        c.doc = Some((date.clone(), rw, render(&text, rw)));
    }
    let doc = &c.doc.as_ref().map(|d| &d.2).cloned().unwrap_or_default();
    let h = cols[1].height.saturating_sub(2) as usize;
    v.page_rows = h;
    v.doc_len = doc.len();
    v.scroll = v.scroll.min(doc.len().saturating_sub(h));
    let shown: Vec<Line> = doc.iter().skip(v.scroll).take(h).cloned().collect();
    let at = if doc.len() > h {
        format!(" · lines {}–{} of {}", v.scroll + 1, (v.scroll + h).min(doc.len()), doc.len())
    } else {
        String::new()
    };
    let title = if v.open {
        vec![bold(format!("READING {} {}", date, weekday(&date)), p.violet), dim(at), dim(" · ↑ ↓ PgUp PgDn scroll · Esc back to the days")]
    } else {
        vec![bold(format!("{} {}", date, weekday(&date)), p.gold), dim(at), dim(" · ↑ ↓ pick a day · Enter to read it")]
    };
    let rc = if v.open { p.violet } else { p.dim };
    f.render_widget(
        Paragraph::new(shown).block(panel(title, rc).padding(ratatui::widgets::Padding::horizontal(1))),
        cols[1],
    );
    draw_footer(f, rows[2], &g.snap, paused);
}

/// A key on this page (menu closed). True when the page used it; q, m, Tab, the page keys, and Esc with no
/// report open, fall through to the screen's own keys.
pub fn key(code: KeyCode) -> bool {
    let n = CACHE.lock().unwrap_or_else(|e| e.into_inner()).list.len();
    let mut v = VIEW.lock().unwrap_or_else(|e| e.into_inner());
    let page = v.page_rows.saturating_sub(2).max(1);
    if v.open {
        match code {
            KeyCode::Up | KeyCode::Char('k') => v.scroll = v.scroll.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => v.scroll += 1,
            KeyCode::PageUp => v.scroll = v.scroll.saturating_sub(page),
            KeyCode::PageDown | KeyCode::Char(' ') => v.scroll += page,
            KeyCode::Home => v.scroll = 0,
            KeyCode::End => v.scroll = v.doc_len,
            KeyCode::Esc | KeyCode::Left | KeyCode::Backspace => v.open = false,
            _ => return false,
        }
    } else {
        let last = n.saturating_sub(1);
        let to = match code {
            KeyCode::Up | KeyCode::Char('k') => v.sel.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => (v.sel + 1).min(last),
            KeyCode::PageUp => v.sel.saturating_sub(page),
            KeyCode::PageDown => (v.sel + page).min(last),
            KeyCode::Home => 0,
            KeyCode::End => last,
            KeyCode::Enter | KeyCode::Right => {
                v.open = n > 0;
                return true;
            }
            _ => return false,
        };
        if to != v.sel {
            v.sel = to;
            v.scroll = 0;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weekdays() {
        assert_eq!(weekday("2026-10-14"), "Wed");
        assert_eq!(weekday("2026-10-15"), "Thu");
        assert_eq!(weekday("2026-03-01"), "Sun");
        assert_eq!(weekday("2024-02-29"), "Thu");
    }

    #[test]
    fn summary_line() {
        let t = "# R\n\n## Summary\n\n- **13 commits** (xrpl-validator 13)\n- **2 engine findings fixed** (F399–F400)\n- Live shadow: 913/913 ledgers fully matched, 91,880 transactions\n\n## Live\n- **9 commits** (x)\n";
        assert_eq!(summarize(t), ("13 commits · 2 fixed · 913/913 ledgers".to_string(), false));
    }

    #[test]
    fn wraps_with_a_hanging_indent() {
        let lines = wrap(inline("aaa bbb **ccc** ddd", Style::new()), 12, vec![Span::raw("  • ")], 4);
        let text: Vec<String> = lines.iter().map(|l| l.spans.iter().map(|s| s.content.to_string()).collect()).collect();
        assert_eq!(text, vec!["  • aaa bbb", "    ccc ddd"]);
    }

    #[test]
    fn tables_keep_columns_and_wrap_the_last() {
        let mut out = Vec::new();
        table(&["| Time | Subject |", "|---|---|", "| 07:26 | one two three four |"], 20, &mut out);
        let text: Vec<String> = out.iter().map(|l| l.spans.iter().map(|s| s.content.to_string()).collect()).collect();
        assert_eq!(text, vec!["  Time   Subject", "  07:26  one two", "         three four"]);
    }

    #[test]
    fn keys_pick_open_scroll_and_close() {
        {
            let mut c = CACHE.lock().unwrap();
            c.scanned = Some(Instant::now());
            c.list = (0..3).map(|k| Entry { date: format!("2026-10-1{k}"), path: PathBuf::new(), summary: String::new(), retro: false }).collect();
        }
        let state = || {
            let v = VIEW.lock().unwrap();
            (v.sel, v.open, v.scroll)
        };
        VIEW.lock().unwrap().doc_len = 100;
        assert!(key(KeyCode::Down) && key(KeyCode::Down) && key(KeyCode::Down));
        assert_eq!(state(), (2, false, 0), "the selection stops at the last day");
        assert!(key(KeyCode::Up));
        assert!(key(KeyCode::Enter));
        assert_eq!(state(), (1, true, 0));
        assert!(key(KeyCode::Down) && key(KeyCode::PageDown));
        assert_eq!(state().2, 1 + 18, "PgDn moves a reader page (20 rows) less two");
        assert!(key(KeyCode::Esc));
        assert_eq!(state().1, false, "Esc closes the report");
        assert!(!key(KeyCode::Esc), "Esc with no report open is the screen's (quit)");
        assert!(!key(KeyCode::Tab) && !key(KeyCode::Char('q')), "Tab and q stay the screen's");
    }
}
