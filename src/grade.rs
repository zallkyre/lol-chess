use shakmaty::{Board, CastlingMode, Chess, Color, Move, Position, Role, Square};

pub fn win_pct(cp: i32) -> f64 {
    let x = (cp.clamp(-1500, 1500) as f64) / 400.0;
    100.0 / (1.0 + 10f64.powf(-x))
}

pub fn from_sq(m: Move) -> Square {
    m.from()
}

pub fn dest_sq(m: Move) -> Square {
    if let Move::Castle { king, rook } = m {
        let (kf, kr) = sq_idx(king);
        let (rf, _) = sq_idx(rook);
        let file = if rf > kf { 6 } else { 2 };
        return sq_at(file, kr).unwrap_or(king);
    }
    m.to()
}

pub fn promo(m: Move) -> Option<Role> {
    m.promotion()
}

pub fn uci_std(m: Move) -> String {
    shakmaty::uci::Uci::from_standard(m).to_string()
}

pub fn landing_squares(m: Move) -> Vec<Square> {
    let mut v = vec![dest_sq(m)];
    if let Move::Castle { king, rook } = m {
        let (kf, kr) = sq_idx(king);
        let (rf, _) = sq_idx(rook);
        if let Some(rsq) = sq_at(if rf > kf { 5 } else { 3 }, kr) {
            v.push(rsq);
        }
    }
    v
}

fn capture_square(m: Move) -> Option<Square> {
    match m {
        Move::EnPassant { from, to } => sq_at(sq_idx(to).0, sq_idx(from).1),
        _ => {
            if m.capture().is_some() {
                Some(dest_sq(m))
            } else {
                None
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Rating {
    Brilliant,
    Great,
    Best,
    Excellent,
    Good,
    Book,
    Inaccuracy,
    Mistake,
    Miss,
    Blunder,
    Forced,
}

impl Rating {
    pub fn key(&self) -> &'static str {
        match self {
            Rating::Brilliant => "brilliant",
            Rating::Great => "great",
            Rating::Best => "best",
            Rating::Excellent => "excellent",
            Rating::Good => "good",
            Rating::Book => "book",
            Rating::Inaccuracy => "inaccuracy",
            Rating::Mistake => "mistake",
            Rating::Miss => "miss",
            Rating::Blunder => "blunder",
            Rating::Forced => "forced",
        }
    }
    pub fn label(&self) -> &'static str {
        match self {
            Rating::Brilliant => "Brilliant",
            Rating::Great => "Great",
            Rating::Best => "Best",
            Rating::Excellent => "Excellent",
            Rating::Good => "Good",
            Rating::Book => "Book",
            Rating::Inaccuracy => "Inaccuracy",
            Rating::Mistake => "Mistake",
            Rating::Miss => "Miss",
            Rating::Blunder => "Blunder",
            Rating::Forced => "Forced",
        }
    }
    pub fn is_bad(&self) -> bool {
        matches!(self, Rating::Inaccuracy | Rating::Mistake | Rating::Miss | Rating::Blunder)
    }
    pub fn colour(&self) -> egui::Color32 {
        use egui::Color32;
        match self {
            Rating::Brilliant => Color32::from_rgb(0x26, 0xc2, 0xa6),
            Rating::Great => Color32::from_rgb(0x5b, 0x8a, 0xf5),
            Rating::Best => Color32::from_rgb(0x81, 0xb6, 0x4c),
            Rating::Excellent => Color32::from_rgb(0x81, 0xb6, 0x4c),
            Rating::Good => Color32::from_rgb(0x95, 0xb7, 0x76),
            Rating::Book => Color32::from_rgb(0xa8, 0x88, 0x8f),
            Rating::Inaccuracy => Color32::from_rgb(0xf7, 0xc0, 0x45),
            Rating::Mistake => Color32::from_rgb(0xe6, 0x91, 0x2c),
            Rating::Miss => Color32::from_rgb(0xff, 0x77, 0x6e),
            Rating::Blunder => Color32::from_rgb(0xfa, 0x41, 0x25),
            Rating::Forced => Color32::from_rgb(0x96, 0xbc, 0x64),
        }
    }
}

pub fn classify(delta: f64) -> Rating {
    if delta <= 3.0 {
        Rating::Excellent
    } else if delta <= 8.0 {
        Rating::Good
    } else if delta <= 14.0 {
        Rating::Inaccuracy
    } else if delta <= 26.0 {
        Rating::Mistake
    } else {
        Rating::Blunder
    }
}

pub fn value(role: Role) -> i32 {
    match role {
        Role::Pawn => 1,
        Role::Knight => 3,
        Role::Bishop => 3,
        Role::Rook => 5,
        Role::Queen => 9,
        Role::King => 10_000,
    }
}

pub fn sq_idx(sq: Square) -> (i32, i32) {
    let s = sq.to_string();
    let b = s.as_bytes();
    ((b[0] as i32 - b'a' as i32), (b[1] as i32 - b'1' as i32))
}

pub fn sq_at(f: i32, r: i32) -> Option<Square> {
    if !(0..8).contains(&f) || !(0..8).contains(&r) {
        return None;
    }
    let s = format!("{}{}", (b'a' + f as u8) as char, (b'1' + r as u8) as char);
    Square::from_ascii(s.as_bytes()).ok()
}

pub struct AttackMap {
    squares: Vec<Square>,
    rays: Vec<Vec<Square>>,
}

impl AttackMap {
    pub fn build(board: &Board) -> Self {
        let mut squares = Vec::with_capacity(32);
        let mut rays = Vec::with_capacity(32);
        for (sq, _) in board.pieces() {
            squares.push(sq);
            rays.push(attacks(board, sq));
        }
        Self { squares, rays }
    }

    pub fn of(&self, sq: Square) -> &[Square] {
        match self.squares.iter().position(|s| *s == sq) {
            Some(i) => &self.rays[i],
            None => &[],
        }
    }

    pub fn attackers_of(&self, board: &Board, target: Square, by: Color) -> Vec<Square> {
        let mut v = Vec::new();
        for (i, sq) in self.squares.iter().enumerate() {
            if let Some(p) = board.piece_at(*sq) {
                if p.color == by && self.rays[i].contains(&target) {
                    v.push(*sq);
                }
            }
        }
        v
    }
}

pub fn attacks(board: &Board, sq: Square) -> Vec<Square> {
    let Some(piece) = board.piece_at(sq) else { return Vec::new() };
    let (f, r) = sq_idx(sq);
    let mut out = Vec::new();
    let mut push = |ff: i32, rr: i32| {
        if let Some(s) = sq_at(ff, rr) {
            out.push(s);
        }
    };
    match piece.role {
        Role::Knight => {
            for (df, dr) in [(1, 2), (2, 1), (2, -1), (1, -2), (-1, -2), (-2, -1), (-2, 1), (-1, 2)] {
                push(f + df, r + dr);
            }
        }
        Role::King => {
            for df in -1..=1 {
                for dr in -1..=1 {
                    if df != 0 || dr != 0 {
                        push(f + df, r + dr);
                    }
                }
            }
        }
        Role::Pawn => {
            let dr = if piece.color == Color::White { 1 } else { -1 };
            push(f - 1, r + dr);
            push(f + 1, r + dr);
        }
        Role::Bishop | Role::Rook | Role::Queen => {
            let dirs: &[(i32, i32)] = match piece.role {
                Role::Bishop => &[(1, 1), (1, -1), (-1, 1), (-1, -1)],
                Role::Rook => &[(1, 0), (-1, 0), (0, 1), (0, -1)],
                _ => &[(1, 1), (1, -1), (-1, 1), (-1, -1), (1, 0), (-1, 0), (0, 1), (0, -1)],
            };
            for &(df, dr) in dirs {
                let (mut ff, mut rr) = (f + df, r + dr);
                while let Some(s) = sq_at(ff, rr) {
                    out.push(s);
                    if board.piece_at(s).is_some() {
                        break;
                    }
                    ff += df;
                    rr += dr;
                }
            }
        }
    }
    out
}

fn collinear(a: Square, b: Square, c: Square) -> bool {
    if a == b || a == c || b == c {
        return false;
    }
    let (af, ar) = sq_idx(a);
    let (bf, br) = sq_idx(b);
    let (cf, cr) = sq_idx(c);
    let d1 = (bf - af, br - ar);
    let d2 = (cf - af, cr - ar);
    d1.0 * d2.1 - d1.1 * d2.0 == 0
}

fn distance(a: Square, b: Square) -> i32 {
    let (af, ar) = sq_idx(a);
    let (bf, br) = sq_idx(b);
    (bf - af).abs().max((br - ar).abs())
}

fn clear_between(board: &Board, a: Square, b: Square) -> bool {
    let (af, ar) = sq_idx(a);
    let (bf, br) = sq_idx(b);
    let (df, dr) = ((bf - af).signum(), (br - ar).signum());
    let (mut f, mut r) = (af + df, ar + dr);
    while (f, r) != (bf, br) {
        let Some(s) = sq_at(f, r) else { return false };
        if board.piece_at(s).is_some() {
            return false;
        }
        f += df;
        r += dr;
    }
    true
}

fn on_ray(origin: Square, near: Square, far: Square) -> bool {
    if !collinear(origin, near, far) {
        return false;
    }
    let (of, or) = sq_idx(origin);
    let (nf, nr) = sq_idx(near);
    let (ff, fr) = sq_idx(far);
    let step = (
        (nf - of).signum().min(1).max(-1),
        (nr - or).signum().min(1).max(-1),
    );
    let far_step = (
        (ff - of).signum().min(1).max(-1),
        (fr - or).signum().min(1).max(-1),
    );
    step == far_step && distance(origin, near) < distance(origin, far)
}

fn is_slider(role: Role) -> bool {
    matches!(role, Role::Bishop | Role::Rook | Role::Queen)
}

fn pinned_targets(board: &Board, mover: Color) -> Vec<Square> {
    let mut out = Vec::new();
    let Some(king) = board.king_of(!mover) else { return out };
    for (sq, p) in board.pieces() {
        if p.color != !mover || p.role == Role::King || value(p.role) < 3 {
            continue;
        }
        for (a, ap) in board.pieces() {
            if ap.color != mover || !is_slider(ap.role) {
                continue;
            }
            if on_ray(a, sq, king) && clear_between(board, a, sq) && clear_between(board, sq, king) {
                out.push(sq);
                break;
            }
        }
    }
    out
}

fn skewered_targets(board: &Board, mover: Color) -> Vec<Square> {
    let mut out = Vec::new();
    let Some(king) = board.king_of(!mover) else { return out };
    for (a, ap) in board.pieces() {
        if ap.color != mover || !is_slider(ap.role) {
            continue;
        }
        if !clear_between(board, a, king) {
            continue;
        }
        for (sq, p) in board.pieces() {
            if p.color == !mover
                && p.role != Role::King
                && value(p.role) >= 3
                && on_ray(a, king, sq)
                && clear_between(board, king, sq)
            {
                out.push(sq);
            }
        }
    }
    out
}

#[derive(Default, Debug, Clone)]
pub struct Tactics {
    pub names: Vec<&'static str>,
}

impl Tactics {
    fn add(&mut self, name: &'static str) {
        if !self.names.contains(&name) {
            self.names.push(name);
        }
    }
    pub fn drop_name(&mut self, name: &str) {
        self.names.retain(|n| *n != name);
    }
}

pub fn detect(before: &Chess, mv: Move, after: &Chess) -> Tactics {
    let mut t = Tactics::default();
    let mover = before.turn();
    let from = from_sq(mv);
    let to = dest_sq(mv);
    let Some(piece) = before.board().piece_at(from) else { return t };
    let them = !mover;

    let map_after = AttackMap::build(after.board());
    let map_before = AttackMap::build(before.board());
    let their_king = after.board().king_of(them);
    let mut moved_attacks: Vec<Square> = Vec::new();
    for sq in landing_squares(mv) {
        for s in map_after.of(sq) {
            if !moved_attacks.contains(s) {
                moved_attacks.push(*s);
            }
        }
    }

    if after.is_checkmate() {
        t.add("checkmate");
    }
    if promo(mv).is_some() {
        t.add("promotion");
    }

    if let Some(king) = their_king {
        if after.is_check() {
            let checkers = map_after.attackers_of(after.board(), king, mover);
            if checkers.len() >= 2 {
                t.add("double check");
            } else if !moved_attacks.contains(&king) {
                t.add("discovered attack");
            }
            if after.is_checkmate() {
                let back_rank = if them == Color::White { 0 } else { 7 };
                if sq_idx(king).1 == back_rank {
                    t.add("back rank");
                }
            }
        }
    }

    let mut hits = 0;
    for s in &moved_attacks {
        if let Some(p) = after.board().piece_at(*s) {
            if p.color == them && (p.role == Role::King || value(p.role) >= 3) {
                hits += 1;
            }
        }
    }
    if hits >= 2 {
        t.add("fork");
    }

    let pins_before = pinned_targets(before.board(), mover);
    let pins_after = pinned_targets(after.board(), mover);
    if pins_after.iter().any(|s| !pins_before.contains(s)) {
        t.add("pin");
    }
    let skew_before = skewered_targets(before.board(), mover);
    let skew_after = skewered_targets(after.board(), mover);
    if skew_after.iter().any(|s| !skew_before.contains(s)) {
        t.add("skewer");
    }

    if let Some(cap_sq) = capture_square(mv) {
        let guarded = map_before.of(cap_sq);
        let now_loose = guarded.iter().any(|s| {
            after
                .board()
                .piece_at(*s)
                .map(|p| {
                    p.color == them
                        && p.role != Role::King
                        && value(p.role) >= 3
                        && map_after.attackers_of(after.board(), *s, them).is_empty()
                        && !map_after.attackers_of(after.board(), *s, mover).is_empty()
                })
                .unwrap_or(false)
        });
        if now_loose && mv.capture().map(value).unwrap_or(0) >= 3 {
            t.add("removing the defender");
        }
    }

    if piece.role != Role::King && value(piece.role) > mv.capture().map(value).unwrap_or(0) + 1 {
        let cheapest = after
            .legal_moves()
            .into_iter()
            .filter(|m| capture_square(*m) == Some(to))
            .filter_map(|m| after.board().piece_at(from_sq(m)).map(|p| value(p.role)))
            .min();
        if let Some(c) = cheapest {
            if c < value(piece.role) {
                t.add("sacrifice");
            }
        }
    }

    t
}

pub fn fen_after(fen: &str, uci: &str) -> Option<(Chess, Move)> {
    use shakmaty::fen::Fen;
    let pos = Fen::from_ascii(fen.as_bytes())
        .ok()?
        .into_position::<Chess>(CastlingMode::Standard)
        .ok()?;
    let u = shakmaty::uci::Uci::from_ascii(uci.as_bytes()).ok()?;
    let mv = u.to_move(&pos).ok()?;
    let next = pos.play(&mv).ok()?;
    Some((next, mv))
}

pub fn san_of(before: &Chess, mv: Move) -> String {
    shakmaty::san::San::from_move(before, mv).to_string()
}

pub fn board_of(fen: &str) -> Chess {
    use shakmaty::fen::Fen;
    Fen::from_ascii(fen.as_bytes())
        .ok()
        .and_then(|f| f.into_position::<Chess>(CastlingMode::Standard).ok())
        .unwrap_or_default()
}

pub fn to_fen(pos: &Chess) -> String {
    shakmaty::fen::Fen::from_position(pos.clone(), shakmaty::EnPassantMode::Legal).to_string()
}

pub fn is_over(pos: &Chess) -> bool {
    pos.is_checkmate()
        || pos.is_stalemate()
        || pos.is_insufficient_material()
        || pos.halfmoves() >= 100
}

pub fn fmt_eval(cp: i32, mate: Option<i32>) -> String {
    if let Some(m) = mate {
        if m > 0 {
            format!("M{m}")
        } else {
            format!("-M{}", m.abs())
        }
    } else {
        format!("{:+.1}", cp as f32 / 100.0)
    }
}

pub fn mate_of(cp: i32) -> Option<i32> {
    if cp.abs() >= 9000 {
        let n = ((10000 - cp.abs()) / 10).max(1);
        Some(if cp > 0 { n } else { -n })
    } else {
        None
    }
}