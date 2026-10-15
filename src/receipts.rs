//! The RECEIPTS page (key `r`): every difference the validator's own checks wrote down since it started, newest
//! first, each in plain words. The logs live beside the shadow receipts; the launcher archives the step logs at every
//! start, and the rest are cut at the first ledger of this run.

use super::*;

/// One source of receipts: what the page calls it, its log, and how its lines read.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Meta,
    Order,
    Peer,
    Validated,
    OwnHash,
    Writer,
    CppChanges,
    CppResults,
}

struct Source {
    kind: Kind,
    label: &'static str,
    env: &'static str,
    file: &'static str,
}

const SOURCES: [Source; 8] = [
    Source { kind: Kind::Meta, label: "STEP 2 · records", env: "XRPL_NATIVE_META_LOG", file: "native_meta.jsonl" },
    Source { kind: Kind::Order, label: "STEP 3 · order", env: "XRPL_NATIVE_ORDER_LOG", file: "native_order.jsonl" },
    Source { kind: Kind::Peer, label: "STEP 4 · peers", env: "XRPL_PEER_FETCH_LOG", file: "peer_fetch.jsonl" },
    Source { kind: Kind::Validated, label: "STEP 4 · validated", env: "XRPL_OWN_VALIDATED_LOG", file: "own_validated.jsonl" },
    Source { kind: Kind::OwnHash, label: "STEP 5 · own hash", env: "XRPL_OWN_HASH_LOG", file: "own_hash.jsonl" },
    Source { kind: Kind::Writer, label: "ENGINE · writer", env: "XRPL_NATIVE_SHADOW_LOG", file: "native_shadow.jsonl" },
    Source { kind: Kind::CppChanges, label: "C++ CHECK · changes", env: "XRPL_FFI_MUTATION_DIVERGENCE_LOG", file: "mutation_divergences.jsonl" },
    Source { kind: Kind::CppResults, label: "C++ CHECK · results", env: "XRPL_FFI_DIVERGENCE_LOG", file: "divergences.jsonl" },
];

/// One receipt, ready to show.
#[derive(Clone)]
pub struct Item {
    pub seq: u64,
    pub source: usize,
    pub tx: String,
    pub what: String,
}

/// What the collector keeps between reads: each log's size and modification time with the lines it held, so an
/// unchanged log is not read again.
#[derive(Clone, Default)]
pub struct Rec {
    /// Per source: (size, mtime, its receipts this run).
    seen: Vec<Option<(u64, SystemTime, Vec<Item>)>>,
    /// The first ledger of this run.
    pub run_start: u64,
    pub read: bool,
}

impl Rec {
    pub fn count(&self, i: usize) -> Option<usize> {
        self.seen.get(i).and_then(|s| s.as_ref()).map(|(_, _, v)| v.len())
    }
    /// The newest receipts first, across every source.
    pub fn latest(&self, n: usize) -> Vec<Item> {
        let mut all: Vec<Item> = self.seen.iter().flatten().flat_map(|(_, _, v)| v.iter().cloned()).collect();
        all.sort_by(|a, b| b.seq.cmp(&a.seq).then(a.source.cmp(&b.source)));
        all.truncate(n);
        all
    }
}

/// One read: every log that changed since `prev`, its receipts from `run_start` on.
pub fn read(cfg: &Cfg, vproc: Option<&ValidatorProc>, prev: &Rec, run_start: u64) -> Rec {
    let data = std::path::Path::new(&cfg.receipts).parent().map(|d| d.to_path_buf()).unwrap_or_default();
    let mut rec = Rec { seen: Vec::with_capacity(SOURCES.len()), run_start, read: true };
    for (i, src) in SOURCES.iter().enumerate() {
        let path = match src.kind {
            Kind::Writer => cfg.receipts.clone(),
            _ => vproc.and_then(|p| p.env(src.env)).unwrap_or_else(|| data.join(src.file).display().to_string()),
        };
        let Ok(m) = fs::metadata(&path) else {
            rec.seen.push(Some((0, UNIX_EPOCH, Vec::new())));
            continue;
        };
        let sig = (m.len(), m.modified().unwrap_or(UNIX_EPOCH));
        if let Some(Some((size, t, items))) = prev.seen.get(i) {
            if (*size, *t) == sig && prev.run_start == run_start {
                rec.seen.push(Some((*size, *t, items.clone())));
                continue;
            }
        }
        // The cumulative C++ logs can be large: their newest 4 MB is far more than a run writes.
        let raw = read_tail(&path, 4 << 20).unwrap_or_default();
        let mut items = Vec::new();
        for line in raw.split(|c| *c == b'\n') {
            let Ok(v) = serde_json::from_slice::<Value>(line) else { continue };
            if let Some(it) = item(i, src.kind, &v) {
                if it.seq >= run_start {
                    items.push(it);
                }
            }
        }
        rec.seen.push(Some((sig.0, sig.1, items)));
    }
    rec
}

fn read_tail(path: &str, max: u64) -> io::Result<Vec<u8>> {
    let mut f = fs::File::open(path)?;
    let len = f.metadata()?.len();
    if len > max {
        f.seek(SeekFrom::Start(len - max))?;
    }
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    if len > max {
        // Drop the partial first line.
        if let Some(i) = buf.iter().position(|c| *c == b'\n') {
            buf.drain(..=i);
        }
    }
    Ok(buf)
}

fn short(h: &str) -> String {
    h.chars().take(12).collect()
}

fn clip(t: &str, n: usize) -> String {
    if t.chars().count() <= n { t.to_string() } else { format!("{}…", t.chars().take(n - 1).collect::<String>()) }
}

/// A receipt line in plain words. None for lines that are not receipts (the writer's daily canary, audit lines).
fn item(source: usize, kind: Kind, v: &Value) -> Option<Item> {
    let seq = v["seq"].as_u64().or_else(|| v["ledger_seq"].as_u64())?;
    let tx = short(v["tx"].as_str().or_else(|| v["tx_hash"].as_str()).unwrap_or(""));
    let what = match kind {
        Kind::Meta => {
            if v.get("tree").is_some() {
                "the ledger's tree of records differs from the header's (it follows from a record that differed)".to_string()
            } else {
                meta_words(v["diff"].as_str()?)
            }
        }
        Kind::Order => {
            let mut w = Vec::new();
            if let Some(o) = v["order"].as_str() {
                w.push(match o.split_once(" at position ") {
                    Some((_, rest)) => format!("our order differed from the network's at position {}", rest.split(':').next().unwrap_or("?")),
                    None => format!("order: {}", clip(o, 60)),
                });
            }
            let n = v["results_n"].as_u64().unwrap_or(0);
            if n > 0 {
                w.push(format!("{n} result{} differed", if n == 1 { "" } else { "s" }));
            }
            let b = v["bytes"]["differ"].as_u64().unwrap_or(0);
            if b > 0 {
                w.push(format!("{b} object{} ended up different", if b == 1 { "" } else { "s" }));
            }
            if v["coins"].is_object() || v["coins"].is_string() {
                w.push("the total XRP differed".to_string());
            }
            if w.is_empty() { "differed (see native_order.jsonl)".to_string() } else { w.join(" · ") }
        }
        Kind::Peer => {
            if v.get("failed").is_some() {
                "no peer gave us the whole ledger in time".to_string()
            } else {
                format!("the peers' copy differed from .39's: {}", clip(&v["differ"].to_string(), 60))
            }
        }
        Kind::Validated => {
            if let Some(n) = v["short"].as_u64() {
                format!("only {n} trusted validations reached us in time (quorum {})", v["quorum"].as_u64().unwrap_or(0))
            } else {
                format!("the trusted validators' quorum named another ledger: {}", short(v["differ"].as_str().unwrap_or("")))
            }
        }
        Kind::OwnHash => {
            let parts: Vec<&str> = v["differs_in"].as_array().into_iter().flatten().filter_map(|x| x.as_str()).collect();
            if parts.is_empty() {
                "our own ledger hash differed from the network's".to_string()
            } else {
                format!("our own ledger hash differed from the network's, in: {}", parts.join(", "))
            }
        }
        Kind::Writer => {
            if v.get("canary").is_some() || v.get("hydrate_audit").is_some() {
                return None;
            }
            if let Some(t) = v["ter_mismatch"].as_array().and_then(|a| a.first()).and_then(|x| x.as_str()) {
                format!("our engine's result differed from the network's: {}", clip(t, 70))
            } else {
                let n = |k: &str| v[k].as_array().map_or(0, |a| a.len());
                format!(
                    "our engine's ledger differed: {} missing, {} extra, {} changed differently",
                    n("missing"),
                    n("extra"),
                    n("byte_diff")
                )
            }
        }
        Kind::CppChanges => {
            let n = |k: &str| v[k].as_array().map_or(0, |a| a.len());
            let (extra, missing) = (n("extra_in_ours"), n("missing_in_ours"));
            let ty = v["tx_type"].as_str().unwrap_or("?");
            match (extra, missing) {
                (e, 0) => format!("{ty}: the C++ copy changed {e} object{} the network's record doesn't list", if e == 1 { "" } else { "s" }),
                (0, m) => format!("{ty}: the network's record lists {m} object{} the C++ copy didn't change", if m == 1 { "" } else { "s" }),
                (e, m) => format!("{ty}: the C++ copy's changes differ ({e} extra, {m} missing)"),
            }
        }
        Kind::CppResults => {
            format!("{}: the C++ copy's result was {}", v["tx_type"].as_str().unwrap_or("?"), v["our_ter"].as_str().unwrap_or("?"))
        }
    };
    Some(Item { seq, source, tx, what })
}

/// Step 2's `diff` text — "<Node> <Type> <key8> <field> ours=… theirs=…" or "… FinalFields ours absent theirs
/// present" — in plain words.
fn meta_words(d: &str) -> String {
    let mut it = d.splitn(5, ' ');
    let (node, ty, key, field, rest) = (it.next().unwrap_or(""), it.next().unwrap_or(""), it.next().unwrap_or(""), it.next().unwrap_or(""), it.next().unwrap_or(""));
    if node == "TransactionResult" {
        return format!("the transaction's result differed ({})", clip(d.trim_start_matches("TransactionResult "), 50));
    }
    let node_words = match node {
        "ModifiedNode" => "changed",
        "DeletedNode" => "deleted",
        "CreatedNode" => "created",
        _ => node,
    };
    let what = format!("{ty} {key} ({node_words})");
    if rest.contains("ours absent theirs present") {
        return format!("{what}: our record left out the {field} the network's has");
    }
    if rest.contains("ours present theirs absent") {
        return format!("{what}: our record has {field} the network's doesn't");
    }
    if field == "PreviousTxnID" && rest.starts_with("ours=null") {
        return format!("{what}: our record lacks the \"last touched by\" mark");
    }
    // A value as the log spells it: a quoted string, a number, or an amount object, shown by its value alone.
    let val = |k: &str| {
        let raw = rest.split(k).nth(1).map(|t| t.split(" theirs=").next().unwrap_or(t).trim()).unwrap_or("");
        match serde_json::from_str::<Value>(raw) {
            Ok(Value::Object(o)) => o.get("value").and_then(|v| v.as_str()).map(str::to_string).unwrap_or_else(|| raw.to_string()),
            Ok(Value::String(t)) => t,
            _ => raw.trim_matches('"').to_string(),
        }
    };
    let (ours, theirs) = (val("ours="), val("theirs="));
    if !ours.is_empty() || !theirs.is_empty() {
        return format!("{what}: {field} ours {} · network {}", clip(&ours, 22), clip(&theirs, 22));
    }
    format!("{what}: {}", clip(field, 40))
}

/// How long ago ledger `seq` closed, at about 3.9 s a ledger.
fn ago_ledgers(now: u64, seq: u64) -> String {
    ago(((now.saturating_sub(seq)) as f64 * 3.9) as u64)
}

pub fn draw(f: &mut Frame, g: &Shared, paused: bool) {
    let p = pal();
    let area = f.area();
    f.render_widget(Block::new().style(Style::new().bg(p.bg)), area);
    let sn = &g.snap;
    let rec = &sn.rec;
    let rows = Layout::vertical([
        Constraint::Length(1), // header
        Constraint::Length(6), // what this page is + the counts
        Constraint::Fill(1),   // the latest receipts
        Constraint::Length(1), // footer
    ])
    .split(area);
    draw_header(f, rows[0], sn);

    // The counts, one per source.
    let mut chips: Vec<Span<'static>> = vec![Span::raw(" ")];
    let mut total = 0usize;
    for (i, src) in SOURCES.iter().enumerate() {
        let n = rec.count(i);
        total += n.unwrap_or(0);
        chips.push(bold(src.label, p.violet));
        chips.push(Span::raw(" "));
        chips.push(match n {
            Some(0) => bold("0", p.good),
            Some(n) => bold(commas(n as u64), p.bad),
            None => dim("—"),
        });
        chips.push(dim("   "));
    }
    let verdict = if !rec.read {
        dim(" reading the logs …")
    } else if total == 0 {
        bold(" Nothing to look at: every check has agreed with the network since the validator started.", p.good)
    } else {
        bold(format!(" {} receipt{} since the validator started. Newest first below.", commas(total as u64), if total == 1 { "" } else { "s" }), p.warn)
    };
    let intro = vec![
        Line::from(dim(" Every difference the validator's own checks wrote down, counted from its last start (each start archives the old ones).")),
        Line::from(dim(format!(" From ledger #{} on. A receipt is a lead, not a fault: each one gets replayed and explained.", commas(rec.run_start)))),
        Line::from(""),
        Line::from(chips),
        Line::from(verdict),
    ];
    f.render_widget(Paragraph::new(intro).block(panel(vec![bold("RECEIPTS", p.pink), dim(" · since the validator started")], p.pink)), rows[1]);

    // The latest receipts.
    let block = panel(vec![bold("LATEST", p.feed), dim(" · newest first · age · where · ledger · transaction · what it means")], p.feed);
    let inner = block.inner(rows[2]);
    f.render_widget(block, rows[2]);
    let now = u(&sn.eng, &["ledger_seq"]);
    let items = rec.latest(inner.height as usize);
    let lines: Vec<Line<'static>> = if items.is_empty() {
        vec![Line::from(""), Line::from(vec![Span::raw("   "), bold("All clear.", p.good), dim(" New receipts appear here as they are written.")])]
    } else {
        items
            .iter()
            .map(|it| {
                Line::from(vec![
                    dim(format!(" {:>8} ", ago_ledgers(now, it.seq))),
                    bold(format!("{:<20}", SOURCES[it.source].label), p.violet),
                    ink(format!("#{:<12}", commas(it.seq))),
                    dim(format!("{:<13}", it.tx)),
                    ink(it.what.clone()),
                ])
            })
            .collect()
    };
    f.render_widget(Paragraph::new(lines), inner);
    draw_footer(f, rows[3], sn, paused);
}
