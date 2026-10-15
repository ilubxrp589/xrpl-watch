//! The DEPLOYS page (key `d`): which build is live and what it calls itself, how its soak is going, what the next
//! cycle carries and how far its tests have got, any Yes/No waiting on James's tap, and the last deploys.
//! The cycle, the soak and the taps live on .39: ~/xrpl-ops/tui_feed.py gathers them each minute into
//! /mnt/xrpl-data/tui_feed.json (with .39's disk wear and the outside agreement scores the SYSTEM and NETWORK pages
//! show). The name comes from the validator itself (its /crawl).

use super::*;

#[derive(Clone, Default)]
pub struct Feed {
    /// .39's feed, Null until it has been read.
    pub v: Value,
    /// What the validator calls itself on the network (its /crawl `build_version`).
    pub version: Option<String>,
    pub read: bool,
}

fn path() -> String {
    std::env::var("TUI_FEED").ok().filter(|v| !v.is_empty()).unwrap_or_else(|| "/mnt/xrpl-data/tui_feed.json".to_string())
}

pub fn read(cfg: &Cfg, prev: &Feed) -> Feed {
    let mut fd = prev.clone();
    if let Some(v) = fs::read(path()).ok().and_then(|raw| serde_json::from_slice::<Value>(&raw).ok()) {
        fd.v = v;
        fd.read = true;
    }
    if let Some(b) = get_json(&format!("{}/crawl", cfg.base)).ok().and_then(|c| c["server"]["build_version"].as_str().map(String::from)) {
        fd.version = Some(b);
    }
    fd
}

pub fn now_s() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

/// How old the feed is, in seconds (None before the first read).
pub fn age(fd: &Feed) -> Option<u64> {
    fd.v["at"].as_f64().map(|a| (now_s() - a).max(0.0) as u64)
}

/// A line saying the feed is late, or nothing while it is fresh (.39 pushes it every minute).
pub fn late(fd: &Feed) -> Option<Line<'static>> {
    let p = pal();
    match age(fd) {
        None if !fd.read => Some(Line::from(dim(" waiting for .39's feed (tui_feed.json) …"))),
        None => None,
        Some(a) if a > 180 => Some(Line::from(sp(format!(" .39's feed is {} old: its xrpl-tui-feed timer may have stopped", ago(a)), p.warn))),
        _ => None,
    }
}

fn kv(k: &str, mut v: Vec<Span<'static>>) -> Line<'static> {
    let mut l = vec![Span::styled(format!(" {k:<13}"), Style::new().fg(pal().violet).add_modifier(Modifier::BOLD))];
    l.append(&mut v);
    Line::from(l)
}

/// "2026-10-14 17:49:16" → "10-14 17:49".
fn short(t: &str) -> String {
    t.get(5..16).unwrap_or(t).to_string()
}

fn cut(t: &str, w: usize) -> String {
    if t.chars().count() <= w { t.to_string() } else { format!("{}…", t.chars().take(w.saturating_sub(1)).collect::<String>()) }
}

fn receipts(n: u64) -> String {
    if n == 1 { "1 receipt".to_string() } else { format!("{n} receipts") }
}

fn since(ts: &Value) -> String {
    ts.as_f64().map(|t| format!(" ({} ago)", ago((now_s() - t).max(0.0) as u64))).unwrap_or_default()
}

pub fn draw(f: &mut Frame, g: &Shared, paused: bool) {
    let p = pal();
    let area = f.area();
    f.render_widget(Block::new().style(Style::new().bg(p.bg)), area);
    let sn = &g.snap;
    let fd = &sn.feed;
    let v = &fd.v;
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Length(8), Constraint::Length(13), Constraint::Fill(1), Constraint::Length(1)]).split(area);
    draw_header(f, rows[0], sn);
    let top = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).split(rows[1]);
    let mid = Layout::horizontal([Constraint::Percentage(62), Constraint::Percentage(38)]).split(rows[2]);
    let bw = (top[1].width as usize).saturating_sub(56).clamp(10, 48);
    let now = now_s();

    // Live now.
    let live = &v["live"];
    let soak = &v["soak"];
    let mut lines = vec![kv("cycle", vec![
        bold(format!("{}", u(live, &["cycle"])), p.good),
        dim(" · commit "),
        ink(s(live, &["main"]).to_string()),
        dim(" · deployed "),
        ink(short(s(live, &["deployed"]))),
        dim(since(&live["deployed_ts"])),
    ])];
    lines.push(kv("calls itself", vec![
        ink(fd.version.clone().unwrap_or_else(|| "…".to_string())),
        dim(" · the name peers see"),
    ]));
    if u(soak, &["cycle"]) == u(live, &["cycle"]) && !soak["match"].is_null() {
        let mm = u(soak, &["mismatch"]);
        lines.push(kv("start-up", vec![
            bold(format!("{} matched", commas(u(soak, &["match"]))), p.good),
            dim(" · "),
            bold(format!("{mm} mismatched"), if mm == 0 { p.good } else { p.bad }),
            dim(format!(" · {} signed · the deploy's own check", commas(u(soak, &["signed"])))),
        ]));
    }
    if let Some(md5) = live["md5"].as_str() {
        lines.push(kv("binary", vec![dim(format!("md5 {md5}"))]));
    }
    if let Some(l) = late(fd) {
        lines.push(l);
    }
    f.render_widget(Paragraph::new(lines).block(panel(vec![bold("LIVE NOW", p.good), dim(" · the build signing on m3060")], p.good)), top[0]);

    // The soak.
    let mut lines: Vec<Line<'static>> = Vec::new();
    // Between a deploy and its soak's start (the start-up checks, about 25 min) the last soak is the old build's.
    let between = soak.is_object() && !live["cycle"].is_null() && u(soak, &["cycle"]) != u(live, &["cycle"]);
    if between {
        lines.push(kv("soak", vec![
            bold(format!("#{}", u(soak, &["n"]) + 1), p.feed),
            dim(format!(" · starts after cycle {}'s start-up checks (~25 min)", u(live, &["cycle"]))),
        ]));
        lines.push(kv("before it", vec![dim(format!("#{} watched cycle {} for {:.1} h", u(soak, &["n"]), u(soak, &["cycle"]), (now - soak["start_ts"].as_f64().unwrap_or(now)).max(0.0) / 3600.0))]));
    } else if soak.is_object() {
        let (st, en) = (soak["start_ts"].as_f64().unwrap_or(now), soak["ends_ts"].as_f64().unwrap_or(now));
        let frac = ((now - st) / (en - st).max(1.0)).clamp(0.0, 1.0);
        let hours = u(soak, &["hours"]).max(1);
        lines.push(kv("soak", vec![
            bold(format!("#{}", u(soak, &["n"])), p.feed),
            dim(" · started "),
            ink(short(s(soak, &["start"]))),
            dim(" · done "),
            ink(short(s(soak, &["ends"]))),
        ]));
        let fill = (frac * bw as f64).round() as usize;
        let mut prog = vec![sp("█".repeat(fill), p.feed), sp("·".repeat(bw - fill.min(bw)), p.dim)];
        if let Some(vd) = soak["verdict"].as_object() {
            let clean = vd.get("clean").and_then(|c| c.as_bool()).unwrap_or(false);
            prog.push(bold(if clean { " ✓ CLEAN".to_string() } else { format!(" ✗ {}", receipts(vd.get("receipts").and_then(|r| r.as_u64()).unwrap_or(0))) }, if clean { p.good } else { p.bad }));
        } else {
            prog.push(bold(format!(" {:.1} of {hours} h", (now - st).max(0.0) / 3600.0), p.feed));
            prog.push(dim(format!(" · {} to go", ago((en - now).max(0.0) as u64))));
        }
        lines.push(kv("", prog));
        if let Some((r, _)) = sn.receipts {
            lines.push(kv("receipts", vec![bold(format!("{r}"), if r == 0 { p.good } else { p.bad }), dim(" this soak, as this screen counts them")]));
        }
        if !soak["failovers"].is_null() {
            let (fo, st2) = (u(soak, &["failovers"]), u(soak, &["fallback_stints"]));
            lines.push(kv("failovers", vec![
                bold(format!("{fo}"), if fo == 0 { p.good } else { p.warn }),
                dim(" to the public server · "),
                bold(format!("{st2}"), if st2 == 0 { p.good } else { p.warn }),
                dim(" stints on it (the 3-hourly check)"),
            ]));
        }
        lines.push(Line::from(dim(" A day with no receipts is a clean soak; a new deploy starts a new soak.")));
    } else {
        lines.push(Line::from(dim(" no soak found in .39's soak.log")));
    }
    f.render_widget(Paragraph::new(lines).block(panel(vec![bold("THE SOAK", p.feed), dim(" · a day of watching after each deploy")], p.feed)), top[1]);

    // The next cycle.
    let nx = &v["next"];
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut title = vec![bold("NEXT", p.violet)];
    if nx.is_object() {
        title.push(bold(format!(" · CYCLE {}", u(nx, &["cycle"])), p.violet));
        if let Some(w) = nx["what"].as_str() {
            title.push(dim(format!(" · {w}")));
        }
        lines.push(kv("commit", vec![ink(s(nx, &["main"]).to_string()), dim(nx["started"].as_str().map(|t| format!(" · tests started {t}")).unwrap_or_default())]));
        let mut road: Vec<Span<'static>> = vec![Span::raw(" ")];
        let mut details = Vec::new();
        for (i, st) in nx["stages"].as_array().into_iter().flatten().enumerate() {
            let name = st[0].as_str().unwrap_or("?").to_string();
            let state = st[1].as_str().unwrap_or("");
            let (mark, c) = match state {
                "GREEN" => ("●", p.good),
                "RED" => ("✗", p.bad),
                "RUNNING" => ("◐", p.warn),
                _ => ("○", p.dim),
            };
            if i > 0 {
                road.push(dim(" ── "));
            }
            road.push(bold(format!("{mark} {name}"), c));
            road.push(dim(format!(" {}", state.to_lowercase())));
            if let Some(d) = st[2].as_str().filter(|d| !d.is_empty()) {
                details.push(kv(&name, vec![dim(d.to_string())]));
            }
        }
        lines.push(Line::from(road));
        lines.extend(details);
        lines.push(Line::from(dim(" what it carries:")));
        let w = (mid[0].width as usize).saturating_sub(16);
        for c in nx["commits"].as_array().into_iter().flatten() {
            lines.push(Line::from(vec![Span::raw("   "), sp(format!("{} ", c[0].as_str().unwrap_or("")), p.gold), ink(cut(c[1].as_str().unwrap_or(""), w))]));
        }
    } else {
        lines.push(Line::from(dim(" no cycle being prepared")));
    }
    f.render_widget(Paragraph::new(lines).block(panel(title, p.violet)), mid[0]);

    // Waiting on James.
    let taps = v["taps"].as_array().cloned().unwrap_or_default();
    let mut lines: Vec<Line<'static>> = Vec::new();
    if taps.is_empty() {
        lines.push(Line::from(vec![bold(" nothing", p.good), dim(" · no Yes/No waiting on your tap")]));
        let ready = nx["stages"].as_array().map_or(false, |a| {
            a.iter().all(|st| st[0].as_str() == Some("deploy") || st[1].as_str() == Some("GREEN")) && a.iter().any(|st| st[0].as_str() == Some("deploy") && st[1].as_str() != Some("GREEN"))
        });
        let tap = &nx["tap"];
        match tap["answer"].as_str() {
            Some("yes") if ready => {
                lines.push(Line::from(""));
                lines.push(Line::from(sp(format!(" you said Yes to #{}: cycle {} is deploying", u(tap, &["n"]), u(nx, &["cycle"])), p.good)));
            }
            Some("no") => {
                lines.push(Line::from(""));
                lines.push(Line::from(sp(format!(" you said No to #{}: cycle {} is held", u(tap, &["n"]), u(nx, &["cycle"])), p.warn)));
            }
            _ if ready && tap.is_null() => {
                lines.push(Line::from(""));
                lines.push(Line::from(sp(format!(" cycle {} has passed every test; its deploy question comes next", u(nx, &["cycle"])), p.warn)));
            }
            _ => {}
        }
    } else {
        for t in &taps {
            lines.push(Line::from(vec![bold(format!(" ✋ #{}", u(t, &["n"])), p.warn), dim(format!(" · asked {}", short(s(t, &["asked"]))))]));
            lines.push(Line::from(ink(format!(" {}", s(t, &["text"])))));
            lines.push(Line::from(""));
        }
    }
    let color = if taps.is_empty() { p.good } else { p.warn };
    f.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(panel(vec![bold("WAITING ON YOU", color), dim(" · Telegram Yes/No")], color)),
        mid[1],
    );

    // The last deploys.
    let mut lines = vec![Line::from(dim(format!(" {:<6}{:<7}{:<10}{:<13}{:<9}{:<32}{}", "soak", "cycle", "commit", "started", "ran", "start-up check", "how the soak ended")))];
    let hist = v["history"].as_array().cloned().unwrap_or_default();
    for (i, h) in hist.iter().enumerate() {
        let mm = u(h, &["mismatch"]);
        let check = if h["match"].is_null() { "—".to_string() } else { format!("{}/{} · {} signed", u(h, &["match"]), mm, u(h, &["signed"])) };
        let (end, c) = match h["verdict"].as_object() {
            Some(vd) if vd.get("clean").and_then(|x| x.as_bool()) == Some(true) => ("✓ clean, no receipts".to_string(), p.good),
            Some(vd) if vd.get("clean").and_then(|x| x.as_bool()) == Some(false) => (format!("✗ {}", receipts(vd.get("receipts").and_then(|x| x.as_u64()).unwrap_or(0))), p.bad),
            _ if i == 0 && u(h, &["cycle"]) == u(live, &["cycle"]) => ("running".to_string(), p.feed),
            _ if i == 0 => (format!("cut short by cycle {}", u(live, &["cycle"])), p.dim),
            _ if h["ran_h"].as_f64().unwrap_or(0.0) >= 24.0 => ("ran its day · no verdict recorded".to_string(), p.dim),
            _ => ("cut short by the next deploy".to_string(), p.dim),
        };
        lines.push(Line::from(vec![
            ink(format!(" {:<6}", format!("#{}", u(h, &["n"])))),
            bold(format!("{:<7}", u(h, &["cycle"])), p.violet),
            sp(format!("{:<10}", s(h, &["main"])), p.gold),
            ink(format!("{:<13}", short(s(h, &["start"])))),
            ink(format!("{:<9}", format!("{:.1} h", h["ran_h"].as_f64().unwrap_or(0.0)))),
            sp(format!("{:<32}", check), if mm == 0 { p.ink } else { p.bad }),
            bold(end, c),
        ]));
    }
    if hist.is_empty() {
        lines.push(Line::from(dim(" no deploys in the feed yet")));
    }
    f.render_widget(Paragraph::new(lines).block(panel(vec![bold("THE LAST DEPLOYS", p.gold), dim(" · newest first, from .39's soak log")], p.gold)), rows[3]);
    draw_footer(f, rows[4], sn, paused);
}
