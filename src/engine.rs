use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Default)]
pub struct Analysis {
    pub best: String,
    pub cp: Option<i32>,
    pub mate: Option<i32>,
    pub second: Option<i32>,
}

impl Analysis {
    pub fn score(&self) -> i32 {
        if let Some(m) = self.mate {
            if m > 0 {
                10000 - m * 10
            } else {
                -10000 - m * 10
            }
        } else {
            self.cp.unwrap_or(0)
        }
    }
}

pub enum Req {
    Search { id: u64, fen: String, moves: Vec<String>, depth: u32, skill: u8 },
    Worst { id: u64, fen: String, moves: Vec<String>, depth: u32 },
    Grade { id: u64, fen: String, moves: Vec<String>, played: String, depth: u32 },
    NewGame,
}

impl Req {
    fn id(&self) -> u64 {
        match self {
            Req::Search { id, .. } => *id,
            Req::Worst { id, .. } => *id,
            Req::Grade { id, .. } => *id,
            Req::NewGame => 0,
        }
    }
}

pub enum Kind {
    Search(Analysis),
    Grade { best: Analysis, played: Option<i32> },
    Failed(&'static str),
    Reset,
}

pub struct Done {
    pub id: u64,
    pub kind: Kind,
}

enum Res {
    Done(Done),
    Cancelled,
    Dead,
}

enum Seek {
    Found(Analysis),
    Cancelled,
    Dead,
}

struct Uci {
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<String>,
}

fn best_move(line: &str) -> Option<String> {
    let rest = line.strip_prefix("bestmove")?;
    let tok = rest.split_whitespace().next().unwrap_or("");
    Some(if tok == "0000" || tok.starts_with('(') {
        String::new()
    } else {
        tok.to_string()
    })
}

fn take_info(line: &str, a: &mut Analysis) {
    let Some(rest) = line.strip_prefix("info ") else { return };
    if rest.starts_with("string") {
        return;
    }
    let toks: Vec<&str> = rest.split_whitespace().collect();
    let mp = toks
        .windows(2)
        .find(|w| w[0] == "multipv")
        .and_then(|w| w[1].parse::<usize>().ok())
        .unwrap_or(1);
    let mut i = 0;
    while i < toks.len() {
        if toks[i] == "score" && i + 2 < toks.len() {
            let bounded = matches!(toks.get(i + 3), Some(&"upperbound") | Some(&"lowerbound"));
            if !bounded {
                if let Ok(val) = toks[i + 2].parse::<i32>() {
                    match toks[i + 1] {
                        "cp" => {
                            if mp <= 1 {
                                a.cp = Some(val);
                                a.mate = None;
                            } else {
                                a.second = Some(val);
                            }
                        }
                        "mate" => {
                            let folded = if val > 0 { 10000 - val * 10 } else { -10000 - val * 10 };
                            if mp <= 1 {
                                a.mate = Some(val);
                                a.cp = None;
                            } else {
                                a.second = Some(folded);
                            }
                        }
                        _ => {}
                    }
                }
            }
            i += 3;
            continue;
        }
        i += 1;
    }
}

#[derive(Default)]
struct Worst {
    depth: i32,
    entries: Vec<(usize, i32, String)>,
}

fn scan_worst(line: &str, w: &mut Worst) {
    let Some(rest) = line.strip_prefix("info ") else { return };
    if rest.starts_with("string") {
        return;
    }
    let toks: Vec<&str> = rest.split_whitespace().collect();
    if toks.iter().any(|t| *t == "upperbound" || *t == "lowerbound") {
        return;
    }
    let mut depth = None;
    let mut mp = 1usize;
    let mut score = None;
    let mut mv: Option<String> = None;
    let mut i = 0;
    while i < toks.len() {
        match toks[i] {
            "depth" => {
                depth = toks.get(i + 1).and_then(|v| v.parse::<i32>().ok());
                i += 2;
            }
            "multipv" => {
                mp = toks.get(i + 1).and_then(|v| v.parse::<usize>().ok()).unwrap_or(1);
                i += 2;
            }
            "score" if i + 2 < toks.len() => {
                if let Ok(v) = toks[i + 2].parse::<i32>() {
                    score = Some(match toks[i + 1] {
                        "mate" => {
                            if v > 0 { 10000 - v * 10 } else { -10000 - v * 10 }
                        }
                        "cp" => v,
                        _ => return,
                    });
                }
                i += 3;
            }
            "pv" => {
                mv = toks.get(i + 1).map(|s| s.to_string());
                break;
            }
            _ => i += 1,
        }
    }
    let (Some(d), Some(sc), Some(m)) = (depth, score, mv) else { return };
    if d > w.depth {
        w.depth = d;
        w.entries.clear();
    }
    if d != w.depth || m.len() < 4 {
        return;
    }
    if let Some(slot) = w.entries.iter_mut().find(|(idx, _, _)| *idx == mp) {
        slot.1 = sc;
        slot.2 = m;
    } else {
        w.entries.push((mp, sc, m));
    }
}

impl Uci {
    fn open(path: &std::path::Path) -> Option<Self> {
        if !path.exists() {
            return None;
        }
        let mut cmd = Command::new(path);
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000);
        }
        let mut child = cmd.spawn().ok()?;
        let stdin = child.stdin.take()?;
        let stdout = child.stdout.take()?;

        let (tx, rx) = channel::<String>();
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        if tx.send(line.clone()).is_err() {
                            break;
                        }
                    }
                }
            }
        });

        let mut u = Uci { child, stdin, rx };
        if !(u.send("uci") && u.wait_for("uciok", 5000)) {
            u.shutdown();
            return None;
        }
        let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
        let threads = threads.clamp(1, 8);
        u.send(&format!("setoption name Threads value {threads}"));
        u.send("setoption name Hash value 256");
        if u.sync() {
            Some(u)
        } else {
            u.shutdown();
            None
        }
    }

    fn send(&mut self, cmd: &str) -> bool {
        writeln!(self.stdin, "{cmd}").is_ok() && self.stdin.flush().is_ok()
    }

    fn sync(&mut self) -> bool {
        self.send("isready") && self.wait_for("readyok", 3000)
    }

    fn wait_for(&mut self, token: &str, ms: u64) -> bool {
        let deadline = Instant::now() + Duration::from_millis(ms);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            match self.rx.recv_timeout(left) {
                Ok(line) if line.contains(token) => return true,
                Ok(_) => continue,
                Err(_) => return false,
            }
        }
    }

    fn new_game(&mut self) -> bool {
        self.send("ucinewgame") && self.sync()
    }

    fn search(
        &mut self,
        req_id: u64,
        latest: &AtomicU64,
        fen: &str,
        moves: &[String],
        depth: u32,
        skill: u8,
        multipv: u8,
        worst: bool,
    ) -> Seek {
        if !self.send(&format!("setoption name MultiPV value {multipv}"))
            || !self.send(&format!("setoption name Skill Level value {skill}"))
            || !self.sync()
        {
            return Seek::Dead;
        }
        let position = if moves.is_empty() {
            format!("position fen {fen}")
        } else {
            format!("position startpos moves {}", moves.join(" "))
        };
        if !self.send(&position) || !self.send(&format!("go depth {depth}")) {
            return Seek::Dead;
        }

        let mut a = Analysis::default();
        let mut worst = Worst::default();
        let deadline = Instant::now() + Duration::from_millis(2000 + u64::from(depth) * 700);
        let mut stop_sent = false;
        let mut cancelled = false;
        let mut grace_until: Option<Instant> = None;

        loop {
            if !stop_sent && latest.load(Ordering::Acquire) != req_id {
                stop_sent = true;
                cancelled = true;
                if !self.send("stop") {
                    return Seek::Dead;
                }
                grace_until = Some(Instant::now() + Duration::from_millis(300));
            }

            let wait = match grace_until {
                Some(g) => {
                    let left = g.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        return Seek::Dead;
                    }
                    left
                }
                None => {
                    let left = deadline.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        stop_sent = true;
                        if !self.send("stop") {
                            return Seek::Dead;
                        }
                        grace_until = Some(Instant::now() + Duration::from_millis(800));
                        Duration::from_millis(800)
                    } else {
                        left
                    }
                }
            }
            .min(Duration::from_millis(40));

            match self.rx.recv_timeout(wait) {
                Ok(line) => {
                    if worst {
                        scan_worst(&line, &mut worst);
                    }
                    if let Some(best) = best_move(&line) {
                        if worst {
                            match worst
                                .entries
                                .iter()
                                .min_by_key(|(_, s, _)| *s)
                                .map(|(_, s, m)| (*s, m.clone()))
                            {
                                Some((score, mv)) => {
                                    a.best = mv;
                                    a.cp = Some(score);
                                    a.mate = None;
                                }
                                None => a.best = best,
                            }
                        } else {
                            a.best = best;
                        }
                        return if cancelled { Seek::Cancelled } else { Seek::Found(a) };
                    }
                    take_info(&line, &mut a);
                }
                Err(RecvTimeoutError::Disconnected) => return Seek::Dead,
                Err(RecvTimeoutError::Timeout) => {}
            }
        }
    }

    fn handle(&mut self, req: Req, latest: &AtomicU64) -> Res {
        match req {
            Req::NewGame => {
                if self.new_game() {
                    Res::Done(Done { id: 0, kind: Kind::Reset })
                } else {
                    Res::Dead
                }
            }
            Req::Search { id, fen, moves, depth, skill } => {
                match self.search(id, latest, &fen, &moves, depth, skill, 1, false) {
                    Seek::Found(a) => Res::Done(Done { id, kind: Kind::Search(a) }),
                    Seek::Cancelled => Res::Cancelled,
                    Seek::Dead => Res::Dead,
                }
            }
            Req::Worst { id, fen, moves, depth } => {
                match self.search(id, latest, &fen, &moves, depth, 20, 50, true) {
                    Seek::Found(a) => Res::Done(Done { id, kind: Kind::Search(a) }),
                    Seek::Cancelled => Res::Cancelled,
                    Seek::Dead => Res::Dead,
                }
            }
            Req::Grade { id, fen, moves, played, depth } => {
                let best = match self.search(id, latest, &fen, &moves, depth, 20, 2, false) {
                    Seek::Found(a) => a,
                    Seek::Cancelled => return Res::Cancelled,
                    Seek::Dead => return Res::Dead,
                };
                let played_score = match fen_after(&fen, &played) {
                    Some(after) => {
                        let mut next = moves;
                        next.push(played);
                        match self.search(id, latest, &after, &next, depth, 20, 1, false) {
                            Seek::Found(r) => Some(-r.score()),
                            Seek::Cancelled => return Res::Cancelled,
                            Seek::Dead => return Res::Dead,
                        }
                    }
                    None => None,
                };
                Res::Done(Done { id, kind: Kind::Grade { best, played: played_score } })
            }
        }
    }

    fn shutdown(&mut self) {
        let _ = self.send("quit");
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub struct Engine {
    tx: Option<Sender<Req>>,
    results: Receiver<Done>,
    next_id: u64,
    latest: Arc<AtomicU64>,
    worker: Option<JoinHandle<()>>,
}

impl Engine {
    pub fn spawn(path: std::path::PathBuf) -> Self {
        let (tx, rx) = channel::<Req>();
        let (done_tx, done_rx) = channel::<Done>();
        let latest = Arc::new(AtomicU64::new(0));
        let latest_worker = latest.clone();

        let worker = thread::spawn(move || {
            let mut uci: Option<Uci> = None;
            while let Ok(mut req) = rx.recv() {
                let mut want_reset = matches!(req, Req::NewGame);
                while let Ok(newer) = rx.try_recv() {
                    if matches!(newer, Req::NewGame) {
                        want_reset = true;
                    } else {
                        req = newer;
                    }
                }
                let id = req.id();

                if id != 0 && latest_worker.load(Ordering::Acquire) != id {
                    let reset_ok = match uci.as_mut() {
                        None => true,
                        Some(u) => !want_reset || u.new_game(),
                    };
                    if !reset_ok {
                        if let Some(mut dead) = uci.take() {
                            dead.shutdown();
                        }
                    }
                    continue;
                }

                let mut attempt = 0u32;
                while uci.is_none() && attempt < 3 {
                    uci = Uci::open(&path);
                    if uci.is_none() {
                        thread::sleep(Duration::from_millis(300 * u64::from(attempt + 1)));
                    }
                    attempt += 1;
                }

                let res = match uci.as_mut() {
                    None => {
                        let _ = done_tx.send(Done {
                            id,
                            kind: Kind::Failed("stockfish not found next to the app"),
                        });
                        continue;
                    }
                    Some(u) => {
                        let ready = !want_reset || matches!(req, Req::NewGame) || u.new_game();
                        if ready {
                            u.handle(req, &latest_worker)
                        } else {
                            Res::Dead
                        }
                    }
                };

                match res {
                    Res::Done(done) => {
                        let _ = done_tx.send(done);
                    }
                    Res::Cancelled => {}
                    Res::Dead => {
                        if let Some(mut dead) = uci.take() {
                            dead.shutdown();
                        }
                        let _ = done_tx.send(Done {
                            id,
                            kind: Kind::Failed("engine stopped responding"),
                        });
                    }
                }
            }
            if let Some(mut u) = uci.take() {
                u.shutdown();
            }
        });

        Self { tx: Some(tx), results: done_rx, next_id: 1, latest, worker: Some(worker) }
    }

    pub fn submit<F>(&mut self, make: F) -> u64
    where
        F: FnOnce(u64) -> Req,
    {
        let id = self.next_id;
        self.next_id += 1;
        self.latest.store(id, Ordering::Release);
        if let Some(tx) = &self.tx {
            let _ = tx.send(make(id));
        }
        id
    }

    pub fn new_game(&mut self) {
        self.next_id += 1;
        self.latest.store(self.next_id, Ordering::Release);
        if let Some(tx) = &self.tx {
            let _ = tx.send(Req::NewGame);
        }
    }

    pub fn cancel(&mut self) {
        self.next_id += 1;
        self.latest.store(self.next_id, Ordering::Release);
    }

    pub fn drain(&self) -> Vec<Done> {
        self.results.try_iter().collect()
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.tx = None;
        if let Some(handle) = self.worker.take() {
            let _ = handle.join();
        }
    }
}

fn fen_after(fen: &str, uci: &str) -> Option<String> {
    use shakmaty::{fen::Fen, CastlingMode, Chess, Position};
    let pos = Fen::from_ascii(fen.as_bytes())
        .ok()?
        .into_position::<Chess>(CastlingMode::Standard)
        .ok()?;
    let u = shakmaty::uci::Uci::from_ascii(uci.as_bytes()).ok()?;
    let mv = u.to_move(&pos).ok()?;
    let next = pos.play(&mv).ok()?;
    Some(Fen::from_position(next, shakmaty::EnPassantMode::Legal).to_string())
}