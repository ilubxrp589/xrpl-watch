//! The NETWORK page (key `n`): who the validator talks to — its peers and their software, the upstream server it
//! still takes ledgers from, the consensus it watches, and the validations and peer ledgers of independence step 4.
//! Peer addresses are never shown. At the bottom, how the public trackers score the validator (from the upstream's feed).

use super::*;

/// Samples the peer-count sparkline keeps (5 s apart: ten minutes).
const KEEP: usize = 120;

#[derive(Clone, Default)]
pub struct Net {
    pub connected: u64,
    pub max: u64,
    pub known: u64,
    pub discovered: u64,
    /// Each peer software version and how many peers run it, most first.
    pub versions: Vec<(String, u64)>,
    pub history: VecDeque<u64>,
    pub read: bool,
}

pub fn read(cfg: &Cfg, prev: &Net) -> Net {
    let mut n = Net { history: prev.history.clone(), ..Net::default() };
    let Ok(v) = get_json(&format!("{}/api/peers", cfg.base)) else { return Net { read: prev.read, ..prev.clone() } };
    n.read = true;
    n.connected = u(&v, &["connected"]);
    n.max = u(&v, &["max"]);
    n.known = u(&v, &["known"]);
    n.discovered = v["discovered"].as_array().map_or(0, |a| a.len() as u64);
    let mut by: BTreeMap<String, u64> = BTreeMap::new();
    for p in v["peer_details"].as_array().into_iter().flatten() {
        *by.entry(p["version"].as_str().unwrap_or("unknown").to_string()).or_default() += 1;
    }
    let mut versions: Vec<(String, u64)> = by.into_iter().collect();
    versions.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    n.versions = versions;
    n.history.push_back(n.connected);
    while n.history.len() > KEEP {
        n.history.pop_front();
    }
    n
}

fn kv(k: &str, mut v: Vec<Span<'static>>) -> Line<'static> {
    let mut l = vec![Span::styled(format!(" {k:<13}"), Style::new().fg(pal().violet).add_modifier(Modifier::BOLD))];
    l.append(&mut v);
    Line::from(l)
}

/// A bar of `n` out of `of`, `width` cells.
fn bar(n: u64, of: u64, width: usize, c: Color) -> Span<'static> {
    let fill = if of == 0 { 0 } else { ((n as f64 / of as f64) * width as f64).round() as usize }.min(width);
    sp(format!("{}{}", "█".repeat(fill), "·".repeat(width - fill)), c)
}

/// "rippled-3.4.1" → ("rippled", "3.4.1").
/// "rippled-3.4.1" -> ("rippled", "3.4.1"); the version starts at the first '-' or '/' followed by a digit, so
/// "rippled-0.0.0-dev+18208d5" keeps its "-dev+…" and "xrpl-rust-validator/0.4.0+…" keeps its name whole.
fn split_version(v: &str) -> (String, String) {
    let b = v.as_bytes();
    match (1..b.len()).find(|&i| (b[i - 1] == b'-' || b[i - 1] == b'/') && b[i].is_ascii_digit()) {
        Some(i) => (v[..i - 1].to_string(), v[i..].to_string()),
        None => (v.to_string(), String::new()),
    }
}

/// The numbers a version starts with, for ordering: "3.4.1" -> [3, 4, 1]; "0.0.0-dev+…" -> [0, 0, 0]. Compared as
/// text, "dev+18208d5" sorted above "3.4.1" and a dev build was shown as the current release (2026-10-15).
fn version_key(ver: &str) -> Vec<u32> {
    let lead: String = ver.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    lead.split('.').filter_map(|x| x.parse().ok()).collect()
}

pub fn draw(f: &mut Frame, g: &Shared, paused: bool) {
    let p = pal();
    let area = f.area();
    f.render_widget(Block::new().style(Style::new().bg(p.bg)), area);
    let sn = &g.snap;
    let net = &sn.net;
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Length(14), Constraint::Length(10), Constraint::Fill(1), Constraint::Length(1)]).split(area);
    draw_header(f, rows[0], sn);
    let top = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).split(rows[1]);
    let low = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).split(rows[2]);
    let bw = (top[0].width as usize).saturating_sub(50).clamp(10, 44);

    // Our peers.
    let mut lines = vec![
        kv("connected", vec![bar(net.connected, net.max, bw, p.feed), bold(format!(" {}", net.connected), p.feed), dim(format!(" of {} allowed", net.max))]),
        kv("", vec![if net.history.len() < 2 { dim("collecting …") } else { sp(spark(&net.history.iter().copied().collect::<Vec<_>>(), (top[0].width as usize).saturating_sub(18).min(KEEP)), p.feed) }, dim("  the last ten minutes")]),
        kv("heard of", vec![ink(commas(net.known)), dim(" servers known · "), ink(commas(net.discovered)), dim(" found this run")]),
        Line::from(""),
        Line::from(vec![dim(" their software, most first:")]),
    ];
    let newest = net.versions.iter().filter(|(v, _)| v.starts_with("rippled") || v.starts_with("xrpld")).map(|(v, _)| version_key(&split_version(v).1)).max();
    for (v, n) in net.versions.iter().take(7) {
        let (name, ver) = split_version(v);
        let name = if name.is_empty() { "unnamed".to_string() } else { name };
        let current = newest.as_ref().is_some_and(|k| !k.is_empty() && *k == version_key(&ver));
        // A peer whose crawl answer carries no server section says nothing about its software (cycle 176 names
        // it "hidden"; before that our crawler wrongly filled in its first peer's version).
        let hidden = name == "hidden" || v == "hidden";
        // An empty answer names no software at all: neither current nor older.
        let blank = v.is_empty();
        let c = if hidden || blank { p.dim } else if name.contains("rust") { p.rust } else if current { p.good } else { p.warn };
        lines.push(Line::from(vec![
            Span::raw("   "),
            bold(format!("{:<28}", format!("{name} {ver}")), c),
            bar(*n, net.connected.max(1), bw.saturating_sub(14).max(8), c),
            ink(format!(" {n:>3}")),
            dim(if hidden { "  hides its version" } else if blank { "  no version given" } else if name.contains("rust") { "  a Rust validator" } else if current { "  current" } else { "  older" }),
        ]));
    }
    if !net.read {
        lines.push(Line::from(dim(" reading the peers …")));
    }
    f.render_widget(Paragraph::new(lines).block(panel(vec![bold("OUR PEERS", p.feed), dim(format!(" · the servers {} is connected to (addresses not shown)", host_name()))], p.feed)), top[0]);

    // The upstream server.
    let up = &sn.up;
    let state = s(up, &["server_state"]);
    let sc = if state == "full" || state == "proposing" { p.good } else { p.warn };
    let mut lines = vec![
        kv("server", vec![ink(up_name()), dim(format!(" · xrpld {}", s(up, &["build_version"])))]),
        kv("state", vec![bold(state.to_string(), sc), dim(if state == "full" { " · in sync with the network" } else { "" })]),
        kv("its peers", vec![ink(commas(u(up, &["peers"])))]),
        kv("its ledgers", vec![dim(s(up, &["complete_ledgers"]).to_string())]),
        kv("validated", vec![ink(format!("#{}", commas(u(up, &["validated_ledger", "seq"])))), dim(format!(" · {} s old", u(up, &["validated_ledger", "age"])))]),
    ];
    if let Some(e) = &sn.up_err {
        lines.push(kv("error", vec![sp(e.chars().take(60).collect::<String>(), p.bad)]));
    }
    lines.push(Line::from(""));
    // 4.3: with XRPL_LEDGER_SOURCE=peers the validator takes nothing from the upstream; at most it checks against it.
    let peers = s(&sn.eng["ledger_source"], &["mode"]) == "peers";
    let title = if peers {
        lines.push(Line::from(dim(format!(" Every ledger the validator processes comes from our peers. {} is a witness only:", up_name()))));
        lines.push(Line::from(dim(" its hashes are checked, never used.")));
        " · the stock server, a witness only"
    } else {
        lines.push(Line::from(dim(format!(" Today every ledger the validator processes comes from {}. Step 4 checks each one against our peers;", up_name()))));
        lines.push(Line::from(dim(" step 4.3 makes the peers the source.")));
        " · the stock server we still take ledgers from"
    };
    f.render_widget(Paragraph::new(lines).block(panel(vec![bold("THE UPSTREAM", p.cpp), dim(title)], p.cpp)), top[1]);

    // Consensus.
    let c = &sn.cons;
    let unl = u(c, &["unl_size"]).max(1);
    let agree = u(c, &["monitor", "agreement", "count"]);
    let lines = vec![
        kv("trusted", vec![ink(format!("{unl} validators")), dim(format!(" on our list · {} heard this round", u(c, &["monitor", "tracked_validators"])))]),
        kv("agreement", vec![bar(agree, unl, bw, p.good), bold(format!(" {agree} of {unl}"), p.good), dim(format!(" ({:.0}%) on one set", agree as f64 * 100.0 / unl as f64))]),
        kv("round", vec![ink(s(c, &["phase"]).to_string()), dim(format!(" · {} transactions on the table", u(c, &["candidate_set_size"])))]),
        kv("proposals", vec![ink(commas(u(c, &["monitor", "total_proposals"]))), dim(" heard since the start")]),
    ];
    f.render_widget(Paragraph::new(lines).block(panel(vec![bold("CONSENSUS", p.violet), dim(" · the round the validators are in")], p.violet)), low[0]);

    // Validations and peer ledgers (independence step 4).
    let ov = &sn.eng["own_validated"];
    let pf = &sn.eng["peer_fetch"];
    let fetched = u(pf, &["fetched"]).max(1);
    let ls = &sn.eng["ledger_source"];
    let mut lines = vec![kv("validations", vec![ink(commas(u(ov, &["validations"]))), dim(" heard · "), ink(commas(u(ov, &["untrusted"]))), dim(" from servers not on our list")])];
    if peers {
        // Nothing of the upstream's to hold our quorum or the peers' ledgers against: what the peers gave, and how.
        let w = &sn.eng["witness"];
        lines.push(kv("our quorum", vec![ink(format!("{} of {}", u(ov, &["quorum"]), u(ov, &["unl"]))), dim(" trusted validators name each ledger we take")]));
        lines.push(Line::from(""));
        lines.push(kv("peer ledgers", vec![bold(commas(u(ls, &["from_peers"])), p.good), dim(" taken from peers · held "), bold(commas(u(ls, &["holds"])), if u(ls, &["holds"]) == 0 { p.good } else { p.bad })]));
        if b(w, &["enabled"]) {
            lines.push(kv("witness", vec![ink(commas(u(w, &["agree"]))), dim(format!(" = {}'s · ", up_name())), bold(commas(u(w, &["differ"])), if u(w, &["differ"]) == 0 { p.good } else { p.bad }), dim(" differ · check only")]));
        }
    } else {
        lines.push(kv("our quorum", vec![bold(commas(u(ov, &["agree"])), p.good), dim(format!(" ledgers called validated as {} did · quorum {} of {}", up_name(), u(ov, &["quorum"]), u(ov, &["unl"])))]));
        lines.push(kv("", vec![dim("differ "), bold(commas(u(ov, &["differ"])), if u(ov, &["differ"]) == 0 { p.good } else { p.bad }), dim(" · short "), bold(commas(u(ov, &["short"])), if u(ov, &["short"]) == 0 { p.good } else { p.bad })]));
        lines.push(Line::from(""));
        lines.push(kv("peer ledgers", vec![bold(commas(u(pf, &["matched"])), p.good), dim(format!(" of {} fetched from peers matched {}'s", commas(u(pf, &["fetched"])), up_name()))]));
    }
    lines.push(kv("", vec![dim(format!("{:.1} requests a ledger · {} peers to ask · {} ms for the last", u(pf, &["requests"]) as f64 / fetched as f64, u(pf, &["peers"]), u(pf, &["ms_last"])))]));
    let title = if peers { " · step 4, live: the source" } else { " · step 4, in shadow" };
    f.render_widget(Paragraph::new(lines).block(panel(vec![bold("VALIDATIONS · PEER LEDGERS", p.gold), dim(title)], p.gold)), low[1]);
    draw_world(f, rows[3], sn, bw);
    draw_footer(f, rows[4], sn, paused);
}

/// How the rest of the network sees the validator: data.xrpl.org's agreement scores for our master key (xrpscan if
/// it is down), fetched by the upstream's feed at most every ten minutes. Everything else on this page is our own view.
fn draw_world(f: &mut Frame, r: Rect, sn: &Snap, bw: usize) {
    let p = pal();
    let w = &sn.feed.v["world"];
    let mut lines: Vec<Line<'static>> = Vec::new();
    if w.is_object() {
        for (k, label) in [("agreement_1h", "1 hour"), ("agreement_24h", "24 hours"), ("agreement_30day", "30 days")] {
            let a = &w[k];
            let score = a["score"].as_str().and_then(|x| x.parse::<f64>().ok()).or_else(|| a["score"].as_f64()).unwrap_or(0.0);
            let c = if score >= 0.99 { p.good } else if score >= 0.9 { p.warn } else { p.bad };
            let mut l = vec![
                bar((score * 1000.0).round() as u64, 1000, bw, c),
                bold(format!(" {:.2}%", score * 100.0), c),
                dim(format!(" · {} missed of {} ledgers", commas(u(a, &["missed"])), commas(u(a, &["total"])))),
            ];
            if a["incomplete"].as_bool() == Some(true) {
                l.push(dim(" · the tracker marks this window incomplete"));
            }
            lines.push(kv(label, l));
        }
        lines.push(Line::from(dim(" missed: a ledger the network validated without a matching validation from us")));
        // One block a day for the last 30 (UTC): green from 99%, amber from 90%, red below; dots where the tracker
        // has no report for the day.
        if let Some(days) = w["days"].as_array().filter(|d| !d.is_empty()) {
            let mut l: Vec<Span<'static>> = Vec::new();
            for d in days {
                let c = match d[1].as_f64() {
                    None => None,
                    Some(sc) if sc >= 0.99 => Some(p.good),
                    Some(sc) if sc >= 0.9 => Some(p.warn),
                    Some(_) => Some(p.bad),
                };
                l.push(match c { Some(c) => sp("██ ", c), None => dim("·· ") });
            }
            let date = |d: &Value| d[0].as_str().and_then(|x| x.get(5..10)).unwrap_or("").to_string();
            l.push(dim(format!(" {} → {}", date(&days[0]), date(&days[days.len() - 1]))));
            lines.push(Line::from(""));
            lines.push(kv("each day", l));
            let worst = days.iter().filter(|d| d[1].as_f64().is_some() && d[3].as_u64().unwrap_or(0) > 0)
                .min_by(|a, b| a[1].as_f64().unwrap_or(1.0).total_cmp(&b[1].as_f64().unwrap_or(1.0)));
            let empty = days.iter().filter(|d| d[1].is_null()).count();
            let mut note = vec![dim(format!("{empty} of {} days without a report", days.len()))];
            if let Some(wd) = worst.filter(|wd| wd[1].as_f64().unwrap_or(1.0) < 0.99) {
                note.push(dim(format!(" · lowest {} at {:.2}% ({} missed of {})", date(wd), wd[1].as_f64().unwrap_or(0.0) * 100.0, commas(wd[2].as_u64().unwrap_or(0)), commas(wd[3].as_u64().unwrap_or(0)))));
            }
            lines.push(kv("", note));
        }
        lines.push(Line::from(""));
        let unl = match &w["unl"] {
            Value::String(list) => vec![bold("yes", p.good), dim(format!(" · {list}"))],
            Value::Bool(true) => vec![bold("yes", p.good)],
            _ => vec![ink("no"), dim(" · not on a published list of trusted validators")],
        };
        lines.push(kv("on a UNL", unl));
        let revoked = w["revoked"].as_bool().unwrap_or(false);
        lines.push(kv("our key", vec![
            ink(format!("{}…", s(w, &["signing_key"]).chars().take(12).collect::<String>())),
            dim(" signing · "),
            bold(if revoked { "REVOKED" } else { "not revoked" }, if revoked { p.bad } else { p.good }),
            dim(format!(" · domain {}", s(w, &["domain"]))),
        ]));
        let ago_s = w["fetched"].as_f64().map(|t| ago((deploys::now_s() - t).max(0.0) as u64)).unwrap_or_default();
        let stale = if w["stale"].as_bool() == Some(true) { " · the last fetch failed, these are older" } else { "" };
        lines.push(Line::from(dim(format!(" from {} · checked {ago_s} ago{stale}", s(w, &["source"])))));
    } else {
        lines.push(Line::from(dim(format!(" waiting for {}'s feed to fetch the outside scores …", up_name()))));
    }
    if let Some(l) = deploys::late(&sn.feed) {
        lines.push(l);
    }
    f.render_widget(
        Paragraph::new(lines).block(panel(vec![bold("AS THE WORLD SEES US", p.pink), dim(" · the public trackers' scores for our validator")], p.pink)),
        r,
    );
}
