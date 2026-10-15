//! The GOVERNANCE page (key `g`): the network's amendments — what is on, what has a majority and when it switches
//! on, what is still off and how this validator and the upstream vote, and which our own engine is ready for. View only:
//! a change to the votes goes to the operator as a Telegram tap.

use super::*;

fn kv(k: &str, mut v: Vec<Span<'static>>) -> Line<'static> {
    let mut l = vec![Span::styled(format!(" {k:<12}"), Style::new().fg(pal().violet).add_modifier(Modifier::BOLD))];
    l.append(&mut v);
    Line::from(l)
}

/// "9 d 3 h" / "5 h 12 min" / "now".
fn left(secs: i64) -> String {
    if secs <= 0 {
        return "any ledger now".to_string();
    }
    let s = secs as u64;
    let (d, h, m) = (s / 86_400, s / 3600 % 24, s / 60 % 60);
    if d > 0 { format!("in {d} d {h} h") } else if h > 0 { format!("in {h} h {m} min") } else { format!("in {m} min") }
}

pub fn draw(f: &mut Frame, g: &Shared, paused: bool) {
    let p = pal();
    let area = f.area();
    f.render_widget(Block::new().style(Style::new().bg(p.bg)), area);
    let sn = &g.snap;
    let am = &sn.ind.amend;
    let feats = &sn.features;
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Length(9), Constraint::Fill(1), Constraint::Length(1)]).split(area);
    draw_header(f, rows[0], sn);
    let top = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).split(rows[1]);
    let low = Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).split(rows[2]);

    let all: Vec<&Value> = feats.as_object().map(|o| o.values().collect()).unwrap_or_default();
    let enabled = all.iter().filter(|f| f["enabled"].as_bool().unwrap_or(false)).count();
    let votes: Vec<String> = am["votes"].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(str::to_string)).collect();
    let guard = &am["guard"];
    let gstatus = s(guard, &["status"]);
    let gap = guard["gap_enabled"].as_array().map_or(0, |a| a.len()) + guard["unknown_enabled"].as_array().map_or(0, |a| a.len());
    let on = |name: &str| all.iter().any(|f| f["name"].as_str() == Some(name) && f["enabled"].as_bool().unwrap_or(false));
    // Step 9 (XRPL_VOTE_PENDING_ONLY): the ballot drops what mainnet has already enabled, as rippled's does.
    let pending_only = am["pending_only"].as_bool().unwrap_or(false);
    let sent = votes.iter().filter(|v| !pending_only || !on(v.as_str())).count();

    // Where things stand.
    let lines = vec![
        kv("on", vec![bold(format!("{enabled}"), p.good), dim(format!(" of the {} amendments the network knows are enabled", all.len()))]),
        kv("our engine", vec![ink(format!("knows {} of the enabled ones' rules", u(guard, &["enabled"]))), dim(format!(" · {} known in all", u(guard, &["known"])))]),
        kv("guard", vec![
            bold(if gstatus == "ok" && gap == 0 { "ok".to_string() } else { gstatus.to_string() }, if gstatus == "ok" && gap == 0 { p.good } else { p.bad }),
            dim(" · no amendment our engine doesn't know is on (one would stop it signing)"),
        ]),
        if pending_only {
            kv("our votes", vec![
                ink(format!("yes on {sent}")),
                dim(format!(" pending only · {} of our {} already on, left off the ballot", votes.len() - sent, votes.len())),
            ])
        } else {
            kv("our votes", vec![ink(format!("yes on {}", votes.len())), dim(" (the validator's own ballot, sent with each validation)")])
        },
        Line::from(""),
        Line::from(dim(" View only. Changing a vote goes to you as a Telegram tap.")),
    ];
    f.render_widget(Paragraph::new(lines).block(panel(vec![bold("GOVERNANCE", p.gold), dim(" · the network's amendments")], p.gold)), top[0]);

    // With a majority: switching on.
    let mut lines: Vec<Line<'static>> = Vec::new();
    for (name, secs, supported) in &sn.amendments {
        let ready = guard["majority_unknown"].as_array().is_none_or(|a| !a.iter().any(|x| x.as_str() == Some(name)));
        lines.push(Line::from(vec![
            Span::raw(" "),
            bold(format!("{name:<30}"), p.gold),
            ink(format!("{:<18}", left(*secs))),
            if ready { bold("our engine ready", p.good) } else { bold("our engine NOT ready", p.bad) },
            dim(if *supported { format!(" · {} supports it", up_name()) } else { format!(" · {} doesn't support it", up_name()) }),
        ]));
    }
    if lines.is_empty() {
        lines.push(Line::from(dim(" none: no amendment has a majority right now")));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(dim(" Switches on two weeks after 80% of trusted validators vote yes, if they keep it.")));
    f.render_widget(Paragraph::new(lines).block(panel(vec![bold("SWITCHING ON", p.warn), dim(" · has a majority, counting down")], p.warn)), top[1]);

    // Not enabled yet.
    let mut off: Vec<&Value> = all.iter().copied().filter(|f| !f["enabled"].as_bool().unwrap_or(false)).collect();
    off.sort_by_key(|f| f["name"].as_str().unwrap_or("").to_string());
    let mut lines = vec![Line::from(vec![dim(format!(" {:<30}{:<16}{:<14}{}", "amendment", format!("{} votes", up_name()), "we vote", "majority"))])];
    for f in off {
        let name = f["name"].as_str().unwrap_or("?").to_string();
        let up_vote = match &f["vetoed"] {
            Value::Bool(true) => ("no", p.dim),
            Value::String(t) if t == "Obsolete" => ("obsolete", p.dim),
            _ => ("yes", p.good),
        };
        let ours = votes.contains(&name);
        lines.push(Line::from(vec![
            Span::raw(" "),
            ink(format!("{name:<30}")),
            sp(format!("{:<16}", up_vote.0), up_vote.1),
            if ours { bold(format!("{:<14}", "yes"), p.good) } else { dim(format!("{:<14}", "no")) },
            if f["majority"].is_number() { bold("yes", p.warn) } else { dim("no") },
        ]));
    }
    f.render_widget(Paragraph::new(lines).block(panel(vec![bold("NOT ENABLED YET", p.violet), dim(format!(" · and how {} and our validator vote", up_name()))], p.violet)), low[0]);

    // Our readiness.
    let mut lines: Vec<Line<'static>> = Vec::new();
    // Only what is still off: an amendment already enabled is no longer pending.
    for r in am["ready_pending"].as_array().into_iter().flatten().filter(|r| !on(r["name"].as_str().unwrap_or(""))) {
        lines.push(Line::from(vec![Span::raw(" "), bold(r["name"].as_str().unwrap_or("?").to_string(), p.good)]));
        lines.push(Line::from(vec![Span::raw("   "), dim(r["evidence"].as_str().unwrap_or("").to_string())]));
    }
    if lines.is_empty() {
        lines.push(Line::from(dim(" nothing pending: what it was ready for is already on")));
    }
    f.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(panel(vec![bold("OUR ENGINE IS READY FOR", p.good), dim(" · and the evidence")], p.good)),
        low[1],
    );
    draw_footer(f, rows[3], sn, paused);
}
