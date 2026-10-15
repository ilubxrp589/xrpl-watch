//! The SYSTEM page (key `s`): the machine the validator runs on — its CPU, memory, disks, temperature and network,
//! and the validator process inside it. From the host sidecar beside the validator (:3779 /metrics every 2 s, /host
//! every minute) and the kernel's own sensors; nothing here asks the validator anything. Below them, .39 (the server
//! every ledger still comes from), from .39's feed (deploys.rs).

use super::*;

/// How many samples the sparklines keep (2 s apart: four minutes).
const KEEP: usize = 120;

#[derive(Clone, Default)]
pub struct Sys {
    pub host: Value,
    pub met: Value,
    /// Each sensor's name and °C.
    pub temps: Vec<(String, f64)>,
    pub cpu: VecDeque<u64>,
    pub vcpu: VecDeque<u64>,
    pub rd: VecDeque<u64>,
    pub wr: VecDeque<u64>,
    pub net_in: VecDeque<u64>,
    pub net_out: VecDeque<u64>,
    host_at: Option<Instant>,
    pub read: bool,
}

fn sidecar() -> String {
    std::env::var("SIDECAR").ok().filter(|v| !v.is_empty()).unwrap_or_else(|| "http://localhost:3779".to_string())
}

fn push(q: &mut VecDeque<u64>, v: u64) {
    q.push_back(v);
    while q.len() > KEEP {
        q.pop_front();
    }
}

/// One read: the fast counters every call, /host once a minute.
pub fn read(prev: &Sys) -> Sys {
    let mut s = prev.clone();
    s.read = true;
    let base = sidecar();
    if s.host_at.map_or(true, |t| t.elapsed() >= Duration::from_secs(60)) {
        if let Ok(v) = get_json(&format!("{base}/host")) {
            s.host = v;
            s.host_at = Some(Instant::now());
        }
    }
    if let Ok(m) = get_json(&format!("{base}/metrics")) {
        let f = |k: &str| m[k].as_f64().unwrap_or(0.0);
        push(&mut s.cpu, (f("cpu_pct") * 10.0) as u64);
        push(&mut s.vcpu, (f("validator_cpu_pct") * 10.0) as u64);
        push(&mut s.rd, (f("disk_read_mbs") * 100.0) as u64);
        push(&mut s.wr, (f("disk_write_mbs") * 100.0) as u64);
        push(&mut s.net_in, (f("net_in_mbs") * 100.0) as u64);
        push(&mut s.net_out, (f("net_out_mbs") * 100.0) as u64);
        s.met = m;
    }
    s.temps.clear();
    if let Ok(dirs) = fs::read_dir("/sys/class/hwmon") {
        let mut all: Vec<(String, f64)> = Vec::new();
        for d in dirs.flatten() {
            let p = d.path();
            let chip = fs::read_to_string(p.join("name")).unwrap_or_default().trim().to_string();
            for i in 1..=8 {
                let Ok(raw) = fs::read_to_string(p.join(format!("temp{i}_input"))) else { continue };
                let Ok(milli) = raw.trim().parse::<f64>() else { continue };
                let label = fs::read_to_string(p.join(format!("temp{i}_label"))).map(|l| l.trim().to_string()).unwrap_or_default();
                let name = match (chip.as_str(), label.as_str()) {
                    ("k10temp", "Tctl") | ("k10temp", "") => "CPU".to_string(),
                    ("nvme", "Composite") | ("nvme", "") => "NVMe".to_string(),
                    (c, "") if c.starts_with("mt79") || c.contains("phy") => "Wi-Fi".to_string(),
                    (c, "") => c.to_string(),
                    (c, l) => format!("{c} {l}"),
                };
                all.push((name, milli / 1000.0));
            }
        }
        all.sort_by(|a, b| a.0.cmp(&b.0));
        s.temps = all;
    }
    s
}

/// A bar `width` cells long, `frac` of it filled, in eighths.
fn meter(frac: f64, width: usize, c: Color) -> Vec<Span<'static>> {
    const PART: [char; 8] = [' ', '▏', '▎', '▍', '▌', '▋', '▊', '▉'];
    let frac = frac.clamp(0.0, 1.0);
    let eighths = (frac * width as f64 * 8.0).round() as usize;
    let full = eighths / 8;
    let mut t: String = "█".repeat(full);
    if full < width {
        t.push(PART[eighths % 8]);
        t.push_str(&"·".repeat(width - full - 1));
    }
    vec![sp(t, c)]
}

/// Green under `warn`, amber to `bad`, red above.
fn level(x: f64, warn: f64, bad: f64) -> Color {
    let p = pal();
    if x >= bad { p.bad } else if x >= warn { p.warn } else { p.good }
}

fn kv(k: &str, mut v: Vec<Span<'static>>) -> Line<'static> {
    let mut l = vec![Span::styled(format!(" {k:<11}"), Style::new().fg(pal().violet).add_modifier(Modifier::BOLD))];
    l.append(&mut v);
    Line::from(l)
}

fn gb(bytes: u64) -> String {
    format!("{:.0} GB", bytes as f64 / 1e9)
}

fn dur(secs: u64) -> String {
    let (d, h, m) = (secs / 86_400, secs / 3600 % 24, secs / 60 % 60);
    if d > 0 { format!("{d} d {h} h") } else if h > 0 { format!("{h} h {m} min") } else { format!("{m} min") }
}

/// A sparkline of `q`, or a note until there are two samples to draw.
fn trend(q: &VecDeque<u64>, width: usize, c: Color) -> Span<'static> {
    if q.len() < 2 { dim("collecting …") } else { sp(spark(&q.iter().copied().collect::<Vec<_>>(), width), c) }
}

/// The newest sample of a series kept ×`scale`.
fn last(q: &VecDeque<u64>, scale: f64) -> f64 {
    q.back().copied().unwrap_or(0) as f64 / scale
}

pub fn draw(f: &mut Frame, g: &Shared, paused: bool) {
    let p = pal();
    let area = f.area();
    f.render_widget(Block::new().style(Style::new().bg(p.bg)), area);
    let sn = &g.snap;
    let sy = &sn.sys;
    let (h, m) = (&sy.host, &sy.met);
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Length(11), Constraint::Length(10), Constraint::Fill(1), Constraint::Length(1)]).split(area);
    draw_header(f, rows[0], sn);
    let top = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).split(rows[1]);
    let low = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).split(rows[2]);
    let bw = (top[0].width as usize).saturating_sub(40).clamp(10, 60);
    let sw = (top[0].width as usize).saturating_sub(16).clamp(10, KEEP);

    // The host.
    let cores = u(h, &["cores"]).max(1);
    let load = h["load"].as_array().cloned().unwrap_or_default();
    let l1 = load.first().and_then(|v| v.as_f64()).unwrap_or(0.0);
    let (ram_used, ram_total) = (m["ram_used_gb"].as_f64().unwrap_or(0.0), m["ram_total_gb"].as_f64().unwrap_or(0.0));
    let swap_used = u(h, &["swap_used"]) as f64 / 1e9;
    let swap_total = (u(h, &["swap_total"]) as f64 / 1e9).max(0.001);
    let cpu = last(&sy.cpu, 10.0);
    let mut lines = vec![
        kv("machine", vec![ink(s(h, &["hostname"]).to_string()), dim(format!(" · {} · kernel {} · {cores} cores", s(h, &["os"]), s(h, &["kernel"])))]),
        kv("up", vec![ink(dur(u(h, &["uptime_s"])))]),
        Line::from(""),
        kv("CPU", { let mut v = meter(cpu / 100.0, bw, level(cpu, 70.0, 90.0)); v.push(bold(format!(" {cpu:>5.1}%"), level(cpu, 70.0, 90.0))); v }),
        kv("", vec![trend(&sy.cpu, sw, p.feed), dim("  the last minutes")]),
        kv("load", { let mut v = meter(l1 / cores as f64, bw, level(l1 / cores as f64, 0.7, 1.0)); v.push(dim(format!(" {}", load.iter().filter_map(|x| x.as_f64()).map(|x| format!("{x:.2}")).collect::<Vec<_>>().join(" · ")))); v }),
        kv("memory", { let mut v = meter(ram_used / ram_total.max(0.001), bw, level(ram_used / ram_total.max(0.001), 0.8, 0.92)); v.push(dim(format!(" {ram_used:.0} of {ram_total:.0} GB"))); v }),
        kv("swap", { let mut v = meter(swap_used / swap_total, bw, level(swap_used / swap_total, 0.5, 0.8)); v.push(dim(format!(" {swap_used:.1} of {swap_total:.0} GB"))); v }),
    ];
    if !sy.read {
        lines.push(Line::from(dim(" reading the host …")));
    }
    f.render_widget(Paragraph::new(lines).block(panel(vec![bold("THE MACHINE", p.feed), dim(" · m3060, where the validator runs")], p.feed)), top[0]);

    // The validator process.
    let vcpu = last(&sy.vcpu, 10.0);
    let vram = m["validator_ram_gb"].as_f64().unwrap_or(0.0);
    let lines = vec![
        kv("process", vec![ink(format!("pid {}", u(h, &["validator_pid"]))), dim(format!(" · {} threads", u(h, &["validator_threads"])))]),
        kv("running", vec![ink(dur(u(h, &["validator_uptime_s"]))), dim(" since its last start")]),
        Line::from(""),
        kv("CPU", { let mut v = meter(vcpu / (100.0 * cores as f64), bw, level(vcpu / cores as f64, 70.0, 90.0)); v.push(bold(format!(" {vcpu:>5.1}%"), p.rust)); v.push(dim(" of one core")); v }),
        kv("", vec![trend(&sy.vcpu, sw, p.rust)]),
        kv("memory", { let mut v = meter(vram / ram_total.max(0.001), bw, level(vram / ram_total.max(0.001), 0.6, 0.8)); v.push(dim(format!(" {vram:.1} of {ram_total:.0} GB"))); v }),
        kv("payments", vec![ink(sn.flow_engine.clone().unwrap_or_else(|| "model".to_string())), dim(" flow engine (XRPL_FLOW_ENGINE)")]),
    ];
    f.render_widget(Paragraph::new(lines).block(panel(vec![bold("THE VALIDATOR PROCESS", p.rust), dim(" · live_viewer")], p.rust)), top[1]);

    // The disks.
    let mut lines: Vec<Line<'static>> = Vec::new();
    if let Some(d) = h["disks"].as_object() {
        for (mount, v) in d {
            let (size, free) = (u(v, &["size"]), u(v, &["free"]));
            let used = size.saturating_sub(free) as f64 / (size.max(1) as f64);
            let mut l = meter(used, bw.saturating_sub(12).max(8), level(used, 0.8, 0.9));
            l.push(dim(format!(" {:.0}% · {} free of {}", used * 100.0, gb(free), gb(size))));
            lines.push(kv(&clip_mount(mount), l));
        }
    }
    lines.push(Line::from(""));
    lines.push(kv("reading", vec![trend(&sy.rd, sw, p.feed), bold(format!(" {:.2} MB/s", last(&sy.rd, 100.0)), p.feed)]));
    lines.push(kv("writing", vec![trend(&sy.wr, sw, p.gold), bold(format!(" {:.2} MB/s", last(&sy.wr, 100.0)), p.gold)]));
    f.render_widget(Paragraph::new(lines).block(panel(vec![bold("DISKS", p.gold), dim(" · how full, and what moves")], p.gold)), low[0]);

    // Temperature and network.
    let mut lines: Vec<Line<'static>> = Vec::new();
    for (name, c) in &sy.temps {
        let col = level(*c, 75.0, 90.0);
        let mut l = meter(*c / 100.0, bw, col);
        l.push(bold(format!(" {c:.0} °C"), col));
        lines.push(kv(name, l));
    }
    if sy.temps.is_empty() {
        lines.push(Line::from(dim(" no sensors readable")));
    }
    lines.push(Line::from(""));
    lines.push(kv("net in", vec![trend(&sy.net_in, sw, p.feed), bold(format!(" {:.2} MB/s", last(&sy.net_in, 100.0)), p.feed)]));
    lines.push(kv("net out", vec![trend(&sy.net_out, sw, p.pink), bold(format!(" {:.2} MB/s", last(&sy.net_out, 100.0)), p.pink)]));
    f.render_widget(Paragraph::new(lines).block(panel(vec![bold("TEMPERATURE · NETWORK", p.pink)], p.pink)), low[1]);
    draw_feed39(f, rows[3], sn, bw);
    draw_footer(f, rows[4], sn, paused);
}

/// .39, where every ledger the validator processes still comes from: its disks' wear (the root xrpl-diskhealth
/// timer reads SMART every 6 h), its memory and load (memory pressure there has stopped the validator before), its
/// xrpld (the upstream's own answer) and how often the validator fell back to the public server this soak.
fn draw_feed39(f: &mut Frame, r: Rect, sn: &Snap, bw: usize) {
    let p = pal();
    let h39 = &sn.feed.v["host39"];
    let mut lines: Vec<Line<'static>> = Vec::new();
    for d in h39["drives"].as_array().into_iter().flatten() {
        let used = d["life_used_pct"].as_f64().unwrap_or(0.0);
        let mount = d["mounts_pct_full"].as_object().and_then(|m| m.iter().next()).map(|(k, v)| (k.clone(), v.as_f64().unwrap_or(0.0)));
        let name = match mount.as_ref().map(|m| m.0.as_str()) {
            Some("/mnt/xrpl-data") => "ledger SSD".to_string(),
            Some("/") => "system SSD".to_string(),
            _ => s(d, &["dev"]).to_string(),
        };
        let c = level(used, 60.0, 80.0);
        let mut l = meter(used / 100.0, bw.saturating_sub(12).max(8), c);
        l.push(bold(format!(" {used:.0}% worn"), c));
        let mut tail = String::new();
        if let Some(m) = d["months_left_estimate"].as_f64() {
            tail += &format!(" · about {m:.1} months left at this pace");
        }
        tail += &format!(" · {:.0} GB written a day · {:.0} TB in all", d["gb_per_day_now"].as_f64().unwrap_or(0.0), d["written_tb"].as_f64().unwrap_or(0.0));
        if let Some((_, full)) = &mount {
            tail += &format!(" · {full:.0}% full");
        }
        if let Some(t) = d["temperature_c"].as_f64() {
            tail += &format!(" · {t:.0} °C");
        }
        l.push(dim(tail));
        lines.push(kv(&name, l));
    }
    let mem = &h39["mem"];
    let (tot, avail) = (u(mem, &["MemTotal"]) as f64 / 1e9, u(mem, &["MemAvailable"]) as f64 / 1e9);
    if tot > 0.0 {
        let used = (tot - avail) / tot;
        let swap = u(mem, &["SwapTotal"]).saturating_sub(u(mem, &["SwapFree"])) as f64 / 1e9;
        let load: Vec<String> = h39["load"].as_array().into_iter().flatten().filter_map(|x| x.as_f64()).map(|x| format!("{x:.2}")).collect();
        let mut l = meter(used, bw.saturating_sub(12).max(8), level(used, 0.8, 0.92));
        l.push(dim(format!(" {:.0} of {tot:.0} GB used · swap {swap:.1} GB · load {} on {} cores", tot - avail, load.join(" · "), u(h39, &["cores"]))));
        lines.push(kv("memory", l));
    }
    let up = &sn.up;
    let state = s(up, &["server_state"]).to_string();
    lines.push(kv("xrpld", vec![
        bold(if state.is_empty() { "unreachable".to_string() } else { state.clone() }, if state == "full" { p.good } else { p.bad }),
        dim(format!(" · ledgers {} · validated {} s ago", s(up, &["complete_ledgers"]), u(up, &["validated_ledger", "age"]))),
    ]));
    let soak = &sn.feed.v["soak"];
    if !soak["failovers"].is_null() {
        let (fo, st) = (u(soak, &["failovers"]), u(soak, &["fallback_stints"]));
        lines.push(kv("failovers", vec![
            bold(format!("{fo}"), if fo == 0 { p.good } else { p.warn }),
            dim(format!(" to the public server this soak · {st} stints on it (the 3-hourly check)")),
        ]));
    }
    if let Some(at) = h39["disk_at"].as_str() {
        lines.push(Line::from(dim(format!(" disk wear read {} (SMART, every 6 h) · the rest each minute", at.get(5..16).unwrap_or(at).replace('T', " ")))));
    }
    if let Some(l) = deploys::late(&sn.feed) {
        lines.push(l);
    }
    f.render_widget(
        Paragraph::new(lines).block(panel(vec![bold(".39 · THE FEED", p.cpp), dim(" · every ledger the validator processes still comes from here")], p.cpp)),
        r,
    );
}

fn clip_mount(m: &str) -> String {
    match m {
        "/" => "/ (system)".to_string(),
        m => m.trim_start_matches("/mnt/").to_string(),
    }
}
