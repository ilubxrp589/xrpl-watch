//! The NETWORK page (key `n`): who the validator talks to — its peers and their software, the upstream server it
//! still takes ledgers from, the consensus it watches, and the validations and peer ledgers of independence step 4.
//! Peer addresses are never shown.

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
fn split_version(v: &str) -> (String, String) {
    match v.rsplit_once(['-', '/']) {
        Some((name, ver)) => (name.to_string(), ver.to_string()),
        None => (v.to_string(), String::new()),
    }
}

pub fn draw(f: &mut Frame, g: &Shared, paused: bool) {
    let p = pal();
    let area = f.area();
    f.render_widget(Block::new().style(Style::new().bg(p.bg)), area);
    let sn = &g.snap;
    let net = &sn.net;
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Fill(1), Constraint::Fill(1), Constraint::Length(1)]).split(area);
    draw_header(f, rows[0], sn);
    let top = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).split(rows[1]);
    let low = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).split(rows[2]);
    let bw = (top[0].width as usize).saturating_sub(50).clamp(10, 44);

    // Our peers.
    let mut lines = vec![
        kv("connected", vec![bar(net.connected, net.max, bw, p.feed), bold(format!(" {}", net.connected), p.feed), dim(format!(" of {} allowed", net.max))]),
        kv("", vec![sp(spark(&net.history.iter().copied().collect::<Vec<_>>(), (top[0].width as usize).saturating_sub(18).min(KEEP)), p.feed), dim("  the last ten minutes")]),
        kv("heard of", vec![ink(commas(net.known)), dim(" servers known · "), ink(commas(net.discovered)), dim(" found this run")]),
        Line::from(""),
        Line::from(vec![dim(" their software, most first:")]),
    ];
    let newest = net.versions.iter().filter(|(v, _)| v.starts_with("rippled") || v.starts_with("xrpld")).map(|(v, _)| split_version(v).1).max();
    for (v, n) in net.versions.iter().take(7) {
        let (name, ver) = split_version(v);
        let current = Some(&ver) == newest.as_ref();
        let c = if name.contains("rust") { p.rust } else if current { p.good } else { p.warn };
        lines.push(Line::from(vec![
            Span::raw("   "),
            bold(format!("{:<28}", format!("{name} {ver}")), c),
            bar(*n, net.connected.max(1), bw, c),
            ink(format!(" {n:>3}")),
            dim(if name.contains("rust") { "  another Rust validator" } else if current { "  current release" } else { "  older" }),
        ]));
    }
    if !net.read {
        lines.push(Line::from(dim(" reading the peers …")));
    }
    f.render_widget(Paragraph::new(lines).block(panel(vec![bold("OUR PEERS", p.feed), dim(" · the servers m3060 is connected to (addresses not shown)")], p.feed)), top[0]);

    // The upstream server.
    let up = &sn.up;
    let state = s(up, &["server_state"]);
    let sc = if state == "full" || state == "proposing" { p.good } else { p.warn };
    let mut lines = vec![
        kv("server", vec![ink(".39"), dim(format!(" · xrpld {}", s(up, &["build_version"])))]),
        kv("state", vec![bold(state.to_string(), sc), dim(if state == "full" { " · in sync with the network" } else { "" })]),
        kv("its peers", vec![ink(commas(u(up, &["peers"])))]),
        kv("its ledgers", vec![dim(s(up, &["complete_ledgers"]).to_string())]),
        kv("validated", vec![ink(format!("#{}", commas(u(up, &["validated_ledger", "seq"])))), dim(format!(" · {} s old", u(up, &["validated_ledger", "age"])))]),
    ];
    if let Some(e) = &sn.up_err {
        lines.push(kv("error", vec![sp(e.chars().take(60).collect::<String>(), p.bad)]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(dim(" Today every ledger the validator processes comes from .39. Step 4 checks each one against our peers;")));
    lines.push(Line::from(dim(" step 4.3 makes the peers the source.")));
    f.render_widget(Paragraph::new(lines).block(panel(vec![bold("THE UPSTREAM", p.cpp), dim(" · the stock server we still take ledgers from")], p.cpp)), top[1]);

    // Consensus.
    let c = &sn.cons;
    let unl = u(c, &["unl_size"]).max(1);
    let agree = u(c, &["monitor", "agreement", "count"]);
    let lines = vec![
        kv("trusted", vec![ink(format!("{unl} validators")), dim(format!(" on our list · {} heard this round", u(c, &["monitor", "tracked_validators"])))]),
        kv("agreement", vec![bar(agree, unl, bw, p.good), bold(format!(" {agree} of {unl}"), p.good), dim(format!(" ({:.0}%) on one transaction set", agree as f64 * 100.0 / unl as f64))]),
        kv("round", vec![ink(s(c, &["phase"]).to_string()), dim(format!(" · {} transactions on the table", u(c, &["candidate_set_size"])))]),
        kv("proposals", vec![ink(commas(u(c, &["monitor", "total_proposals"]))), dim(" heard since the start")]),
    ];
    f.render_widget(Paragraph::new(lines).block(panel(vec![bold("CONSENSUS", p.violet), dim(" · the round the validators are in")], p.violet)), low[0]);

    // Validations and peer ledgers (independence step 4).
    let ov = &sn.eng["own_validated"];
    let pf = &sn.eng["peer_fetch"];
    let fetched = u(pf, &["fetched"]).max(1);
    let lines = vec![
        kv("validations", vec![ink(commas(u(ov, &["validations"]))), dim(" heard · "), ink(commas(u(ov, &["untrusted"]))), dim(" from servers not on our list")]),
        kv("our quorum", vec![bold(commas(u(ov, &["agree"])), p.good), dim(format!(" ledgers called validated as .39 did · quorum {} of {}", u(ov, &["quorum"]), u(ov, &["unl"])))]),
        kv("", vec![dim("differ "), bold(commas(u(ov, &["differ"])), if u(ov, &["differ"]) == 0 { p.good } else { p.bad }), dim(" · short "), bold(commas(u(ov, &["short"])), if u(ov, &["short"]) == 0 { p.good } else { p.bad })]),
        Line::from(""),
        kv("peer ledgers", vec![bold(commas(u(pf, &["matched"])), p.good), dim(format!(" of {} fetched from peers matched .39's", commas(u(pf, &["fetched"]))))]),
        kv("", vec![dim(format!("{:.1} requests a ledger · {} peers to ask · {} ms for the last", u(pf, &["requests"]) as f64 / fetched as f64, u(pf, &["peers"]), u(pf, &["ms_last"])))]),
    ];
    f.render_widget(Paragraph::new(lines).block(panel(vec![bold("VALIDATIONS · PEER LEDGERS", p.gold), dim(" · step 4, in shadow")], p.gold)), low[1]);
    draw_footer(f, rows[3], sn, paused);
}
