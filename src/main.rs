mod assets;
mod board;
mod engine;
mod grade;

use board::{Badge, BoardState};
use engine::{Analysis, Engine, Kind, Req};
use grade::{Rating, Tactics};
use shakmaty::{Chess, Color, Move, Position};
use std::sync::OnceLock;
use std::time::Instant;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1180.0, 800.0])
            .with_min_inner_size([900.0, 640.0])
            .with_title("LOL Chess"),
        ..Default::default()
    };
    eframe::run_native("LOL Chess", options, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}

fn now() -> f64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs_f64()
}

fn engine_binary() -> std::path::PathBuf {
    let dir = assets::root().join("engine");
    let direct = dir.join(if cfg!(windows) { "stockfish.exe" } else { "stockfish" });
    if direct.exists() {
        return direct;
    }
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_lowercase();
            let archive = [".zip", ".gz", ".tar", ".7z", ".txt", ".md", ".json"]
                .iter()
                .any(|ext| name.ends_with(ext));
            if name.starts_with("stockfish") && !archive {
                return path;
            }
        }
    }
    direct
}

const BOOK: &[&str] = &[
    "e4 e5 Nf3 Nc6 Bb5",
    "e4 e5 Nf3 Nc6 Bc4",
    "e4 e5 Nf3 Nc6 d4",
    "e4 e5 Nf3 Nf6",
    "e4 e5 Nc3",
    "e4 e5 f4",
    "e4 e5 Bc4",
    "e4 c5 Nf3",
    "e4 c5 c3",
    "e4 c5 Nc3",
    "e4 e6 d4",
    "e4 c6 d4",
    "e4 d5",
    "e4 d6 d4",
    "e4 g6 d4",
    "e4 Nf6 e5",
    "d4 d5 c4",
    "d4 Nf6 c4",
    "d4 Nf6 Nf3",
    "d4 Nf6 Bf4",
    "d4 d5 Bf4",
    "d4 f5",
    "c4 e5",
    "c4 c5",
    "Nf3 d5",
    "Nf3 Nf6",
];

const APP_VERSION: &str = "0.4.0";

fn is_newer(remote: &str, local: &str) -> bool {
    let parse = |s: &str| -> (u32, u32, u32) {
        let mut it = s
            .trim()
            .trim_start_matches('v')
            .split('.')
            .filter_map(|p| p.trim().parse().ok());
        (
            it.next().unwrap_or(0),
            it.next().unwrap_or(0),
            it.next().unwrap_or(0),
        )
    };
    parse(remote) > parse(local)
}

const VERSION_URL: &str = "https://raw.githubusercontent.com/zallkyre/lol-chess/main/VERSION";
const INSTALL_CMD: &str = "irm https://raw.githubusercontent.com/zallkyre/lol-chess/main/install.ps1 | iex";

fn spawn_update_check() -> std::sync::mpsc::Receiver<String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        #[cfg(windows)]
        let out = {
            use std::os::windows::process::CommandExt;
            std::process::Command::new("powershell")
                .args([
                    "-NoProfile",
                    "-WindowStyle",
                    "Hidden",
                    "-Command",
                    &format!("(Invoke-RestMethod -UseBasicParsing '{VERSION_URL}')"),
                ])
                .creation_flags(0x08000000)
                .output()
        };
        #[cfg(not(windows))]
        let out = std::process::Command::new("sh")
            .args(["-c", &format!("curl -fsSL '{VERSION_URL}'")])
            .output();
        if let Ok(o) = out {
            let v = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if !v.is_empty() {
                let _ = tx.send(v);
            }
        }
    });
    rx
}

#[derive(Clone, Copy)]
enum BotKind {
    Worst,
    Skill(u8),
}

struct BotDef {
    name: &'static str,
    blurb: &'static str,
    rating: i32,
    kind: BotKind,
}

const BOTS: &[BotDef] = &[
    BotDef { name: "Hopeless", blurb: "plays the worst move on the board", rating: 400, kind: BotKind::Worst },
    BotDef { name: "Beginner", blurb: "greedy, blind to everything else", rating: 800, kind: BotKind::Skill(0) },
    BotDef { name: "Casual", blurb: "sees one move ahead", rating: 1100, kind: BotKind::Skill(5) },
    BotDef { name: "Club", blurb: "punishes loose pieces", rating: 1500, kind: BotKind::Skill(8) },
    BotDef { name: "Strong", blurb: "steady tactics", rating: 1900, kind: BotKind::Skill(12) },
    BotDef { name: "Master", blurb: "rarely misses", rating: 2300, kind: BotKind::Skill(16) },
    BotDef { name: "Full", blurb: "engine strength", rating: 2700, kind: BotKind::Skill(20) },
];

struct Speed {
    name: &'static str,
    base: f64,
    inc: f64,
}

const SPEEDS: &[Speed] = &[
    Speed { name: "Bullet 1+0", base: 60.0, inc: 0.0 },
    Speed { name: "Blitz 3+2", base: 180.0, inc: 2.0 },
    Speed { name: "Blitz 5+3", base: 300.0, inc: 3.0 },
    Speed { name: "Rapid 10+0", base: 600.0, inc: 0.0 },
    Speed { name: "Classical 30+0", base: 1800.0, inc: 0.0 },
    Speed { name: "No clock", base: 0.0, inc: 0.0 },
];

struct Clock {
    white: f64,
    black: f64,
    inc: f64,
}

fn fmt_clock(secs: f64) -> String {
    let s = secs.max(0.0);
    if s >= 60.0 {
        format!("{}:{:02}", (s / 60.0).floor() as i64, (s % 60.0) as i64)
    } else {
        format!("{:.1}", s)
    }
}

struct App {
    pos: Chess,
    human: Color,
    board: BoardState,
    assets: assets::Assets,
    engine: Engine,
    theme: usize,
    set: usize,
    skill: u8,
    hopeless: bool,
    bot_name: String,
    bot_rating: i32,
    show_bots: bool,
    bot_setup: Option<usize>,
    setup_color: u8,
    setup_speed: usize,
    player_elo: f64,
    elo_games: u32,
    records: std::collections::HashMap<String, (u32, u32, u32)>,
    clock: Option<Clock>,
    scored: bool,
    last_delta: Option<i32>,
    update: Option<String>,
    update_rx: Option<std::sync::mpsc::Receiver<String>>,

    san_list: Vec<String>,
    move_pairs: Vec<(String, String)>,

    coach: String,
    rating: Option<Rating>,
    tactics: Tactics,
    badge: Option<Badge>,

    pending_grade: Option<u64>,
    pending_bot: Option<u64>,
    pending_hint: Option<u64>,
    pending_fen: Option<String>,
    pending_uci: Option<String>,
    uci_history: Vec<String>,
    eval_history: Vec<i32>,
    eval_cp: i32,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        let mut style = (*cc.egui_ctx.style()).clone();
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        cc.egui_ctx.set_style(style);

        let engine_path = engine_binary();
        let human = Color::White;
        let mut app = Self {
            pos: Chess::default(),
            human,
            board: BoardState { orientation: human, ..Default::default() },
            assets: assets::Assets::new(),
            engine: Engine::spawn(engine_path),
            theme: 0,
            set: 0,
            skill: 8,
            hopeless: false,
            bot_name: String::new(),
            bot_rating: 1500,
            show_bots: false,
            bot_setup: None,
            setup_color: 0,
            setup_speed: 1,
            player_elo: 1500.0,
            elo_games: 0,
            records: std::collections::HashMap::new(),
            clock: None,
            scored: false,
            last_delta: None,
            update: None,
            update_rx: Some(spawn_update_check()),
            san_list: Vec::new(),
            move_pairs: Vec::new(),
            coach: "Your move. I'm watching.".to_string(),
            rating: None,
            tactics: Tactics::default(),
            badge: None,
            pending_grade: None,
            pending_bot: None,
            pending_hint: None,
            pending_fen: None,
            pending_uci: None,
            uci_history: Vec::new(),
            eval_history: vec![20],
            eval_cp: 20,
        };
        app.load_profile();
        app
    }

    fn apply(&mut self, mv: Move, animate: bool) {
        let from = mv.from();
        let to = grade::dest_sq(mv);
        if let Some(piece) = self.pos.board().piece_at(from) {
            if animate {
                self.board
                    .start_slide(from, to, piece.color == Color::White, piece.role, now());
            } else {
                self.board.anim = None;
            }
        }

        let san = grade::san_of(&self.pos, mv);
        let before_fen = grade::to_fen(&self.pos);
        let uci = grade::uci_std(mv);

        let Ok(next) = self.pos.play(&mv) else { return };
        self.pos = next;
        if let Some(c) = &mut self.clock {
            if !self.scored {
                let mover = !self.pos.turn();
                if mover == Color::White {
                    c.white += c.inc;
                } else {
                    c.black += c.inc;
                }
            }
        }
        self.check_end();

        self.board.selected = None;
        self.board.drag = None;
        self.board.hint = None;
        self.pending_hint = None;
        self.board.last = Some((from, to));
        self.san_list.push(san);
        self.uci_history.push(uci.clone());
        self.rebuild_pairs();

        let fen_for_grade = before_fen.clone();
        let played = uci.clone();
        let history: Vec<String> = self
            .uci_history
            .get(..self.uci_history.len().saturating_sub(1))
            .map(|s| s.to_vec())
            .unwrap_or_default();
        let id = self.engine.submit(move |id| Req::Grade {
            id,
            fen: fen_for_grade,
            moves: history,
            played,
            depth: 14,
        });
        self.pending_grade = Some(id);
        self.pending_fen = Some(before_fen);
        self.pending_uci = Some(uci);
    }

    fn rebuild_pairs(&mut self) {
        self.move_pairs.clear();
        let mut i = 0;
        while i < self.san_list.len() {
            let white = self.san_list[i].clone();
            let black = self.san_list.get(i + 1).cloned().unwrap_or_default();
            self.move_pairs.push((white, black));
            i += 2;
        }
    }

    fn request_bot(&mut self) {
        let fen = grade::to_fen(&self.pos);
        let moves = self.uci_history.clone();
        let id = if self.hopeless {
            self.engine.submit(move |id| Req::Worst { id, fen, moves, depth: 8 })
        } else {
            let skill = self.skill;
            self.engine.submit(move |id| Req::Search { id, fen, moves, depth: 12, skill })
        };
        self.pending_bot = Some(id);
    }

    fn on_grade(&mut self, best: Analysis, played: Option<i32>) {
        self.pending_grade = None;
        let Some(fen) = self.pending_fen.take() else {
            return;
        };
        let uci = self.pending_uci.take();

        let Some(played_cp) = played else {
            self.coach = "I couldn't grade that one - the engine slipped.".into();
            return;
        };

        let before = grade::board_of(&fen);
        let mover = before.turn();
        let best_cp = best.score();
        let delta = (grade::win_pct(best_cp) - grade::win_pct(played_cp)).max(0.0);
        let best_win = grade::win_pct(best_cp);
        let played_win = grade::win_pct(played_cp);

        let mut tactics = Tactics::default();
        if let Some(u) = &uci {
            if let Some((after, mv)) = grade::fen_after(&fen, u) {
                tactics = grade::detect(&before, mv, &after);
            }
        }

        let is_best = uci.as_deref() == Some(best.best.as_str()) || played_cp >= best_cp;
        let rating = if before.legal_moves().len() <= 1 {
            Rating::Forced
        } else if is_book_line(&self.san_list) {
            Rating::Book
        } else if is_best {
            if tactics.names.contains(&"sacrifice") && played_win >= 45.0 {
                Rating::Brilliant
            } else if best
                .second
                .map(|s| best_win - grade::win_pct(s) >= 15.0)
                .unwrap_or(false)
            {
                Rating::Great
            } else {
                Rating::Best
            }
        } else if best_win >= 80.0 && (35.0..60.0).contains(&played_win) {
            Rating::Miss
        } else if delta <= 3.0 && tactics.names.contains(&"sacrifice") && played_win >= 45.0 {
            Rating::Brilliant
        } else {
            grade::classify(delta)
        };

        let san = self.san_list.last().cloned().unwrap_or_default();
        self.rating = Some(rating);
        if rating.is_bad() {
            tactics.drop_name("sacrifice");
        }
        self.coach = coach_line(rating, &tactics, san, delta);
        self.tactics = tactics;

        if let Some((_, to)) = self.board.last {
            self.badge = Some(Badge { sq: to, cls: rating.key(), until: now() + 3.5 });
        }

        self.eval_cp = if mover == Color::White { played_cp } else { -played_cp };
        while self.eval_history.len() > self.san_list.len() {
            self.eval_history.pop();
        }
        self.eval_history.push(self.eval_cp);

        if self.pos.turn() != self.human && !grade::is_over(&self.pos) {
            self.request_bot();
        }
    }

    fn on_bot(&mut self, a: Analysis) {
        self.pending_bot = None;
        if a.best.is_empty() {
            return;
        }
        let bot_side = self.pos.turn();
        if let Ok(u) = shakmaty::uci::Uci::from_ascii(a.best.as_bytes()) {
            if let Ok(mv) = u.to_move(&self.pos) {
                let san = grade::san_of(&self.pos, mv);
                let (from, to) = (mv.from(), grade::dest_sq(mv));
                let moving = self
                    .pos
                    .board()
                    .piece_at(from)
                    .map(|p| (p.color == Color::White, p.role));
                if let Some((white, role)) = moving {
                    self.board.start_slide(from, to, white, role, now());
                }
                if let Ok(next) = self.pos.play(&mv) {
                    self.board.last = Some((from, to));
                    self.badge = None;
                    self.pos = next;
        if let Some(c) = &mut self.clock {
            if !self.scored {
                let mover = !self.pos.turn();
                if mover == Color::White {
                    c.white += c.inc;
                } else {
                    c.black += c.inc;
                }
            }
        }
        self.check_end();
                    self.uci_history.push(grade::uci_std(mv));
                    self.san_list.push(san);
                    self.rebuild_pairs();
                }
            }
        }
        let s = a.score();
        self.eval_cp = if bot_side == Color::White { s } else { -s };
        while self.eval_history.len() > self.san_list.len() {
            self.eval_history.pop();
        }
        self.eval_history.push(self.eval_cp);
        self.rating = None;
        self.tactics = Tactics::default();
        if grade::is_over(&self.pos) {
            self.coach = "Game over. Hit New game when you want another.".into();
        }
    }

    fn on_hint(&mut self, a: Analysis) {
        self.pending_hint = None;
        let Ok(u) = shakmaty::uci::Uci::from_ascii(a.best.as_bytes()) else { return };
        let Ok(mv) = u.to_move(&self.pos) else { return };
        self.board.hint = Some((mv.from(), grade::dest_sq(mv)));
        self.coach = format!("Try {}.", grade::san_of(&self.pos, mv));
    }

    fn undo(&mut self) {
        self.engine.cancel();
        if self.san_list.is_empty() {
            return;
        }
        while !self.san_list.is_empty() {
            self.san_list.pop();
            if let Some(p) = replay(&self.san_list) {
                if p.turn() == self.human {
                    break;
                }
            }
        }
        self.uci_history.truncate(self.san_list.len());
        self.rebuild_pairs();
        self.pos = replay(&self.san_list).unwrap_or_default();
        self.pending_grade = None;
        self.pending_bot = None;
        self.pending_hint = None;
        self.board.last = None;
        self.board.selected = None;
        self.board.hint = None;
        self.board.anim = None;
        self.board.drag = None;
        self.eval_history.truncate(self.san_list.len() + 1);
        self.eval_cp = *self.eval_history.last().unwrap_or(&20);
        self.rating = None;
        self.tactics = Tactics::default();
        self.badge = None;
        self.coach = "Back to you.".into();
        if self.pos.turn() != self.human && !grade::is_over(&self.pos) {
            self.request_bot();
        }
    }

    fn new_game(&mut self) {
        self.bot_name.clear();
        self.bot_rating = 1500;
        self.clock = None;
        self.scored = false;
        self.last_delta = None;
        self.new_game_as(self.human);
    }

    fn new_game_as(&mut self, human: Color) {
        self.human = human;
        self.pos = Chess::default();
        self.san_list.clear();
        self.uci_history.clear();
        self.move_pairs.clear();
        self.board = BoardState { orientation: human, ..Default::default() };
        self.rating = None;
        self.tactics = Tactics::default();
        self.badge = None;
        self.coach = "Fresh board. Your move.".into();
        self.eval_cp = 20;
        self.eval_history = vec![20];
        self.pending_grade = None;
        self.pending_bot = None;
        self.pending_hint = None;
        self.engine.new_game();
        if self.pos.turn() != self.human && !grade::is_over(&self.pos) {
            self.request_bot();
        }
    }

    fn hint(&mut self) {
        if self.pos.turn() != self.human {
            return;
        }
        let fen = grade::to_fen(&self.pos);
        let moves = self.uci_history.clone();
        let id = self.engine.submit(move |id| Req::Search { id, fen, moves, depth: 16, skill: 20 });
        self.pending_hint = Some(id);
    }

    fn thinking(&self) -> bool {
        self.pending_grade.is_some() || self.pending_bot.is_some() || self.pending_hint.is_some()
    }

    fn profile_path() -> std::path::PathBuf {
        assets::root().join("profile.ini")
    }

    fn load_profile(&mut self) {
        let Ok(text) = std::fs::read_to_string(Self::profile_path()) else {
            return;
        };
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let (k, v) = (k.trim(), v.trim());
            match k {
                "elo" => self.player_elo = v.parse().unwrap_or(1500.0),
                "games" => self.elo_games = v.parse().unwrap_or(0),
                _ => {
                    if let Some(name) = k.strip_prefix("rec.") {
                        let mut it = v.split('/');
                        let w = it.next().and_then(|x| x.parse().ok()).unwrap_or(0);
                        let l = it.next().and_then(|x| x.parse().ok()).unwrap_or(0);
                        let d = it.next().and_then(|x| x.parse().ok()).unwrap_or(0);
                        self.records.insert(name.to_string(), (w, l, d));
                    }
                }
            }
        }
    }

    fn save_profile(&self) {
        let mut text = format!("elo={:.1}\ngames={}\n", self.player_elo, self.elo_games);
        for (name, (w, l, d)) in &self.records {
            text.push_str(&format!("rec.{name}={w}/{l}/{d}\n"));
        }
        let _ = std::fs::write(Self::profile_path(), text);
    }

    fn score_game(&mut self, score: f64) {
        if self.scored {
            return;
        }
        self.scored = true;
        let before = self.player_elo;
        let k = if self.elo_games < 30 { 40.0 } else { 20.0 };
        let expected = 1.0 / (1.0 + 10f64.powf((self.bot_rating as f64 - before) / 400.0));
        self.player_elo = before + k * (score - expected);
        self.elo_games += 1;
        self.last_delta = Some((self.player_elo - before).round() as i32);
        let rec = self.records.entry(self.bot_name.clone()).or_insert((0, 0, 0));
        if score > 0.75 {
            rec.0 += 1;
        } else if score < 0.25 {
            rec.1 += 1;
        } else {
            rec.2 += 1;
        }
        self.clock = None;
        self.board.locked = true;
        self.save_profile();
    }

    fn check_end(&mut self) {
        if self.scored || self.bot_name.is_empty() || !grade::is_over(&self.pos) {
            return;
        }
        let score = if self.pos.is_checkmate() {
            if self.pos.turn() == self.human {
                0.0
            } else {
                1.0
            }
        } else {
            0.5
        };
        self.score_game(score);
        self.coach = match score {
            1.0 => "Checkmate. That one is yours.".to_string(),
            0.0 => "Checkmate. The bot got there.".to_string(),
            _ => "Drawn.".to_string(),
        };
    }

    fn start_bot_game(&mut self, i: usize) {
        if i >= BOTS.len() {
            return;
        }
        let bot = &BOTS[i];
        let human = match self.setup_color {
            0 => Color::White,
            1 => Color::Black,
            _ => {
                let nanos = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.subsec_nanos())
                    .unwrap_or(0);
                if nanos % 2 == 0 {
                    Color::White
                } else {
                    Color::Black
                }
            }
        };
        self.bot_name = bot.name.to_string();
        self.bot_rating = bot.rating;
        self.hopeless = matches!(bot.kind, BotKind::Worst);
        if let BotKind::Skill(s) = bot.kind {
            self.skill = s;
        }
        let speed = &SPEEDS[self.setup_speed.min(SPEEDS.len().saturating_sub(1))];
        self.clock = if speed.base > 0.0 {
            Some(Clock { white: speed.base, black: speed.base, inc: speed.inc })
        } else {
            None
        };
        self.scored = false;
        self.last_delta = None;
        self.new_game_as(human);
        self.board.locked = false;
        self.coach = format!("{} ({}) - your move.", bot.name, bot.rating);
    }

    fn cycle_theme(&mut self) {
        if !assets::THEMES.is_empty() {
            self.theme = (self.theme + 1) % assets::THEMES.len();
        }
    }

    fn cycle_pieces(&mut self) {
        if !assets::PIECE_SETS.is_empty() {
            self.set = (self.set + 1) % assets::PIECE_SETS.len();
        }
    }
}

fn replay(sans: &[String]) -> Option<Chess> {
    let mut pos = Chess::default();
    for san in sans {
        let parsed = shakmaty::san::San::from_ascii(san.as_bytes()).ok()?;
        let mv = parsed.to_move(&pos).ok()?;
        pos = pos.play(&mv).ok()?;
    }
    Some(pos)
}

fn is_book_line(sans: &[String]) -> bool {
    if sans.is_empty() {
        return false;
    }
    BOOK.iter().any(|line| {
        let toks: Vec<&str> = line.split(' ').collect();
        toks.len() >= sans.len() && sans.iter().zip(toks.iter()).all(|(a, b)| a.as_str() == *b)
    })
}

fn coach_line(rating: Rating, t: &Tactics, san: String, delta: f64) -> String {
    let has = |name: &str| t.names.iter().any(|n| *n == name);
    let listed = t.names.join(", ");
    match rating {
        Rating::Brilliant => {
            if has("sacrifice") {
                format!("{san} - you gave material away on purpose and it works. That's a real sacrifice.")
            } else {
                format!("{san} is brilliant. Clean, and I don't say that often.")
            }
        }
        Rating::Great => format!("{san} was the only move holding this together. Good find."),
        Rating::Best => {
            if t.names.is_empty() {
                format!("{san} - top move.")
            } else {
                format!("{san} - {listed}. Engine's choice too.")
            }
        }
        Rating::Excellent => format!("{san} is excellent. Nothing to fix."),
        Rating::Good => format!("{san} is solid. Small concession, nothing to punish."),
        Rating::Book => format!("{san} - theory. You're still in the book."),
        Rating::Forced => format!("{san} was forced. Your king had to go there."),
        Rating::Inaccuracy => {
            if t.names.is_empty() {
                format!("{san} - slightly off, a better move was available.")
            } else {
                format!("{san} is a slip. {listed} was there for you.")
            }
        }
        Rating::Mistake => format!(
            "{san} isn't working. You gave up about {:.0}% of your winning chances.",
            delta
        ),
        Rating::Miss => {
            if t.names.is_empty() {
                format!("{san} wasted a winning position. There was a tactic.")
            } else {
                format!("{san} got away from you. {listed} was sitting right there.")
            }
        }
        Rating::Blunder => format!("{san} - that one hurts. The position flipped."),
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(rx) = self.update_rx.take() {
            match rx.try_recv() {
                Ok(v) => {
                    let v = v.trim().to_string();
                    if !v.is_empty() && is_newer(&v, APP_VERSION) {
                        self.update = Some(v);
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => self.update_rx = Some(rx),
                Err(_) => {}
            }
        }

        let mut timeout: Option<bool> = None;
        if let Some(c) = &mut self.clock {
            if !self.scored && !grade::is_over(&self.pos) {
                let dt = ctx.input(|i| i.stable_dt).min(0.25) as f64;
                if self.pos.turn() == Color::White {
                    c.white -= dt;
                } else {
                    c.black -= dt;
                }
                if c.white <= 0.0 {
                    timeout = Some(self.human == Color::White);
                } else if c.black <= 0.0 {
                    timeout = Some(self.human == Color::Black);
                }
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        if let Some(human_lost) = timeout {
            self.score_game(if human_lost { 0.0 } else { 1.0 });
            self.coach = if human_lost {
                "Flag fell. You lost on time.".to_string()
            } else {
                "Bot flagged. You win on time.".to_string()
            };
        }
        let finished = self.engine.drain();
        for done in finished {
            if Some(done.id) == self.pending_grade {
                match done.kind {
                    Kind::Grade { best, played } => self.on_grade(best, played),
                    Kind::Failed(why) => {
                        self.pending_grade = None;
                        self.coach = format!("Offline: {why}.");
                        if self.pos.turn() != self.human && !grade::is_over(&self.pos) {
                            self.request_bot();
                        }
                    }
                    Kind::Search(_) => {}
                    Kind::Reset => {}
                }
            } else if Some(done.id) == self.pending_bot {
                match done.kind {
                    Kind::Search(a) => self.on_bot(a),
                    Kind::Failed(why) => {
                        self.pending_bot = None;
                        self.coach = format!("I couldn't move: {why}.");
                    }
                    Kind::Grade { .. } => {}
                    Kind::Reset => {}
                }
            } else if Some(done.id) == self.pending_hint {
                match done.kind {
                    Kind::Search(a) => self.on_hint(a),
                    Kind::Failed(why) => {
                        self.pending_hint = None;
                        self.coach = format!("No hint: {why}.");
                    }
                    Kind::Grade { .. } => {}
                    Kind::Reset => {}
                }
            }
        }

        let expired = self.badge.as_ref().map(|b| now() >= b.until).unwrap_or(false);
        if expired {
            self.badge = None;
        }
        let animating = self.board.anim.is_some();
        let badge_live = self.badge.is_some();
        if animating || self.thinking() || badge_live {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }

        egui::TopBottomPanel::top("top")
            .frame(
                egui::Frame::NONE
                    .fill(egui::Color32::from_rgb(0x26, 0x24, 0x21))
                    .inner_margin(egui::Margin::symmetric(14, 8)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("LOL").strong().size(18.0).color(egui::Color32::from_rgb(0x81, 0xb6, 0x4c)));
                    ui.label(egui::RichText::new("CHESS").strong().size(18.0).color(egui::Color32::from_rgb(0xd6, 0xd6, 0xd6)));
                    ui.add_space(12.0);
                    let status = if grade::is_over(&self.pos) {
                        "game over"
                    } else if self.thinking() {
                        "thinking..."
                    } else if self.pos.turn() == self.human {
                        "your move"
                    } else {
                        "engine idle"
                    };
                    ui.label(egui::RichText::new(status).size(12.0).color(egui::Color32::from_rgb(0x9a, 0x9a, 0x9a)));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        #[cfg(windows)]
                        if let Some(v) = self.update.clone() {
                            if ui.button(format!("Update {v}")).clicked() {
                                let _ = std::process::Command::new("powershell")
                                    .args([
                                        "-NoProfile",
                                        "-ExecutionPolicy",
                                        "Bypass",
                                        "-Command",
                                        INSTALL_CMD,
                                    ])
                                    .spawn();
                                self.coach = "Updating in a new window...".to_string();
                            }
                        }
                        if ui.button("Bots").clicked() {
                            self.show_bots = true;
                        }
                        if !self.scored && !self.bot_name.is_empty() && ui.button("Resign").clicked() {
                            self.score_game(0.0);
                            self.coach = "Resigned.".to_string();
                        }
                        if self.pos.turn() != self.human && !self.thinking() && !grade::is_over(&self.pos) {
                            if ui.button("Retry bot").clicked() {
                                self.request_bot();
                            }
                        }
                        if ui.button("New game").clicked() {
                            self.new_game();
                        }
                        let other = if self.human == Color::White { "Play black" } else { "Play white" };
                        if ui.button(other).clicked() {
                            let human = if self.human == Color::White { Color::Black } else { Color::White };
                            self.new_game_as(human);
                        }
                        if ui.button("Undo").clicked() {
                            self.undo();
                        }
                        if ui.button("Flip").clicked() {
                            self.board.orientation = if self.board.orientation == Color::White {
                                Color::Black
                            } else {
                                Color::White
                            };
                        }
                        if ui.button("Hint").clicked() {
                            self.hint();
                        }
                    });
                });
            });

        egui::SidePanel::right("side")
            .exact_width(330.0)
            .frame(
                egui::Frame::NONE
                    .fill(egui::Color32::from_rgb(0x1c, 0x1b, 0x19))
                    .inner_margin(egui::Margin::same(14)),
            )
            .show(ctx, |ui| {
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    ui.checkbox(&mut self.hopeless, "hopeless bot");
                    ui.add_enabled(!self.hopeless, egui::Slider::new(&mut self.skill, 0..=20).text("strength"));
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(format!("elo {:.0}", self.player_elo))
                                .size(11.0)
                                .color(egui::Color32::from_gray(150)),
                        );
                        if let Some(d) = self.last_delta {
                            ui.label(
                                egui::RichText::new(format!("{d:+}"))
                                    .size(11.0)
                                    .color(egui::Color32::from_gray(175)),
                            );
                        }
                        if ui.button("Board").clicked() {
                            self.cycle_theme();
                        }
                        if ui.button("Pieces").clicked() {
                            self.cycle_pieces();
                        }
                        ui.label(
                            egui::RichText::new(format!("eval {}", grade::fmt_eval(self.eval_cp, grade::mate_of(self.eval_cp))))
                                .size(11.0)
                                .color(egui::Color32::from_rgb(0x8a, 0x86, 0x80)),
                        );
                    });

                    egui::ScrollArea::vertical()
                        .id_salt("moves")
                        .stick_to_bottom(true)
                        .auto_shrink([false, false])
                        .max_height((ui.available_height() - 30.0).max(80.0))
                        .show(ui, |ui| {
                            for (i, (w, b)) in self.move_pairs.iter().enumerate() {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(format!("{:>2}.", i + 1))
                                            .color(egui::Color32::from_rgb(0x7a, 0x76, 0x70))
                                            .monospace(),
                                    );
                                    ui.label(egui::RichText::new(w.as_str()).size(14.0));
                                    ui.label(
                                        egui::RichText::new(b.as_str())
                                            .size(14.0)
                                            .color(egui::Color32::from_rgb(0xd0, 0xcd, 0xc6)),
                                    );
                                });
                            }
                        });

                    ui.add_space(10.0);
                    egui::Frame::NONE
                        .fill(egui::Color32::from_rgb(0x26, 0x24, 0x21))
                        .corner_radius(egui::CornerRadius::same(10))
                        .inner_margin(egui::Margin::same(12))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(
                                egui::RichText::new(self.coach.as_str())
                                    .size(13.5)
                                    .color(egui::Color32::from_rgb(0xe4, 0xe1, 0xda)),
                            );
                        });

                    if !self.tactics.names.is_empty() {
                        ui.horizontal_wrapped(|ui| {
                            for name in &self.tactics.names {
                                egui::Frame::NONE
                                    .fill(egui::Color32::from_rgb(0x2f, 0x2c, 0x28))
                                    .corner_radius(egui::CornerRadius::same(5))
                                    .inner_margin(egui::Margin::symmetric(7, 2))
                                    .show(ui, |ui| {
                                        ui.label(
                                            egui::RichText::new(*name)
                                                .size(11.5)
                                                .color(egui::Color32::from_rgb(0xc9, 0xc4, 0xb8)),
                                        );
                                    });
                            }
                        });
                    }

                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Coach").strong().size(15.0));
                        if let Some(r) = self.rating {
                            let col = r.colour();
                            egui::Frame::NONE
                                .fill(col.linear_multiply(0.22))
                                .corner_radius(egui::CornerRadius::same(6))
                                .inner_margin(egui::Margin::symmetric(8, 3))
                                .show(ui, |ui| {
                                    ui.label(egui::RichText::new(r.label()).color(col).strong());
                                });
                        }
                    });
                });
            });

        if self.show_bots {
            let mut open = true;
            egui::Window::new("Bots")
                .open(&mut open)
                .default_width(380.0)
                .show(ctx, |ui| {
                    ui.label(
                        egui::RichText::new(format!("your rating {:.0}", self.player_elo)).strong(),
                    );
                    ui.separator();
                    for (i, bot) in BOTS.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.label(egui::RichText::new(bot.name).strong());
                                let rec = self.records.get(bot.name).copied().unwrap_or((0, 0, 0));
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{} - {} - {}/{}/{}",
                                        bot.rating, bot.blurb, rec.0, rec.1, rec.2
                                    ))
                                    .size(11.0)
                                    .color(egui::Color32::from_gray(150)),
                                );
                            });
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.button("Play").clicked() {
                                        self.bot_setup = Some(i);
                                    }
                                },
                            );
                        });
                        ui.separator();
                    }
                });
            self.show_bots = open;
        }

        if let Some(i) = self.bot_setup {
            let mut open = true;
            let mut start = false;
            if let Some(bot) = BOTS.get(i) {
                egui::Window::new(format!("Play {}", bot.name))
                    .collapsible(false)
                    .resizable(false)
                    .open(&mut open)
                    .show(ctx, |ui| {
                        ui.label(egui::RichText::new(format!("rated {}", bot.rating)).color(egui::Color32::from_gray(160)));
                        ui.separator();
                        ui.label("you play");
                        ui.radio_value(&mut self.setup_color, 0, "white");
                        ui.radio_value(&mut self.setup_color, 1, "black");
                        ui.radio_value(&mut self.setup_color, 2, "random");
                        ui.separator();
                        ui.label("speed");
                        let sel = self.setup_speed.min(SPEEDS.len().saturating_sub(1));
                        egui::ComboBox::from_id_salt("speed")
                            .selected_text(SPEEDS[sel].name)
                            .show_ui(ui, |ui| {
                                for (k, s) in SPEEDS.iter().enumerate() {
                                    ui.selectable_value(&mut self.setup_speed, k, s.name);
                                }
                            });
                        ui.separator();
                        if ui.button("Start").clicked() {
                            start = true;
                        }
                    });
            }
            if start {
                self.start_bot_game(i);
                self.bot_setup = None;
            } else if !open {
                self.bot_setup = None;
            }
        }

        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(egui::Color32::from_rgb(0x16, 0x15, 0x12))
                    .inner_margin(egui::Margin::same(18)),
            )
            .show(ctx, |ui| {
                if let Some(c) = &self.clock {
                    let (you, bot) = if self.human == Color::White {
                        (c.white, c.black)
                    } else {
                        (c.black, c.white)
                    };
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(format!("you {}", fmt_clock(you)))
                                .monospace()
                                .size(15.0),
                        );
                        ui.label(
                            egui::RichText::new(format!("bot {}", fmt_clock(bot)))
                                .monospace()
                                .size(15.0)
                                .color(egui::Color32::from_gray(150)),
                        );
                    });
                }
                ui.horizontal(|ui| {
                    let h = ui.available_height();
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, h), egui::Sense::hover());
                    let white_share = (grade::win_pct(self.eval_cp) / 100.0) as f32;
                    ui.painter()
                        .rect_filled(rect, egui::CornerRadius::same(3), egui::Color32::from_rgb(0x3a, 0x3a, 0x3a));
                    let wh = rect.height() * white_share;
                    ui.painter().rect_filled(
                        egui::Rect::from_min_size(
                            egui::pos2(rect.left(), rect.bottom() - wh),
                            egui::vec2(rect.width(), wh),
                        ),
                        egui::CornerRadius::same(3),
                        egui::Color32::from_rgb(0xdd, 0xdd, 0xdd),
                    );

                    let theme = &assets::THEMES[self.theme.min(assets::THEMES.len().saturating_sub(1))];
                    let click = self.board.show(
                        ui,
                        &self.pos,
                        &mut self.assets,
                        self.set,
                        theme,
                        self.human,
                        now(),
                        self.badge.as_ref(),
                    );

                    if let Some((mv, animate)) = click {
                        if self.pos.turn() == self.human {
                            self.apply(mv, animate);
                        }
                    }
                });
            });
    }
}