use crate::assets::{Assets, Theme};
use crate::grade::{dest_sq, from_sq, is_over, promo, sq_at, sq_idx};
use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use shakmaty::{Chess, Color, Move, Position, Role, Square};

pub struct Drag {
    pub from: Square,
    pub grab: Vec2,
}

pub struct Anim {
    pub white: bool,
    pub role: Role,
    
    pub from_sq: Square,
    pub to_sq: Square,
    pub t0: f64,
    pub dur: f64,
}

pub struct Badge {
    pub sq: Square,
    pub cls: &'static str,
    pub until: f64,
}

pub struct BoardState {
    pub orientation: Color,
    pub selected: Option<Square>,
    pub drag: Option<Drag>,
    pub anim: Option<Anim>,
    pub last: Option<(Square, Square)>,
    pub hint: Option<(Square, Square)>,
    pub arrows: Vec<(Square, Square)>,
    pub arrow_from: Option<Square>,
    pub show_coords: bool,
    pub hover: Option<Square>,
    pub origin: Pos2,
    pub cell: f32,
}

impl Default for BoardState {
    fn default() -> Self {
        Self {
            orientation: Color::White,
            selected: None,
            drag: None,
            anim: None,
            last: None,
            hint: None,
            arrows: Vec::new(),
            arrow_from: None,
            show_coords: true,
            hover: None,
            origin: Pos2::ZERO,
            cell: 64.0,
        }
    }
}

pub struct Layout {
    pub origin: Pos2,
    pub cell: f32,
}

impl Layout {
    pub fn rect(&self, sq: Square, orientation: Color) -> Rect {
        let (f, r) = sq_idx(sq);
        let col = if orientation == Color::White { f } else { 7 - f };
        let row = if orientation == Color::White { 7 - r } else { r };
        Rect::from_min_size(
            Pos2::new(
                self.origin.x + col as f32 * self.cell,
                self.origin.y + row as f32 * self.cell,
            ),
            Vec2::splat(self.cell),
        )
    }

    pub fn square_at(&self, p: Pos2, orientation: Color) -> Option<Square> {
        let dx = p.x - self.origin.x;
        let dy = p.y - self.origin.y;
        if dx < 0.0 || dy < 0.0 {
            return None;
        }
        let col = (dx / self.cell).floor() as i32;
        let row = (dy / self.cell).floor() as i32;
        if !(0..8).contains(&col) || !(0..8).contains(&row) {
            return None;
        }
        let f = if orientation == Color::White { col } else { 7 - col };
        let r = if orientation == Color::White { row } else { 7 - row };
        sq_at(f, r)
    }
}

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

fn legal_targets(pos: &Chess, from: Square) -> Vec<Move> {
    pos.legal_moves().into_iter().filter(|m| from_sq(*m) == from).collect()
}

fn target_move(pos: &Chess, from: Square, to: Square) -> Option<Move> {
    let moves = legal_targets(pos, from);
    moves
        .iter()
        .find(|m| dest_sq(**m) == to && promo(**m).map_or(true, |r| r == Role::Queen))
        .copied()
        .or_else(|| moves.iter().find(|m| dest_sq(**m) == to).copied())
}

impl BoardState {
    pub fn start_slide(&mut self, from: Square, to: Square, white: bool, role: Role, now: f64) {
        self.anim = Some(Anim { white, role, from_sq: from, to_sq: to, t0: now, dur: 0.14 });
    }

    #[allow(clippy::too_many_arguments)]
    pub fn show(
        &mut self,
        ui: &mut Ui,
        pos: &Chess,
        assets: &mut Assets,
        set: usize,
        theme: &Theme,
        human: Color,
        now: f64,
        badge: Option<&Badge>,
    ) -> Option<(Move, bool)> {
        let side = ui.available_width().min(ui.available_height()).floor().max(64.0);
        let (resp, painter) = ui.allocate_painter(Vec2::splat(side), Sense::click_and_drag());
        let lay = Layout { origin: resp.rect.min, cell: side / 8.0 };
        self.origin = lay.origin;
        self.cell = lay.cell;

        let board = pos.board();
        let pointer = ui.input(|i| i.pointer.latest_pos().or_else(|| i.pointer.interact_pos()));
        let (sec_pressed, sec_released) = ui.input(|i| {
            (i.pointer.secondary_pressed(), i.pointer.secondary_released())
        });

        let mut out: Option<(Move, bool)> = None;
        let can_move = pos.turn() == human && !is_over(pos);
        self.hover = pointer.and_then(|p| lay.square_at(p, self.orientation));

        let ours = |sq: Square| board.piece_at(sq).map(|pc| pc.color == human).unwrap_or(false);

        if sec_pressed && resp.hovered() {
            self.arrow_from = self.hover;
        }
        if sec_released {
            match (self.arrow_from.take(), self.hover) {
                (Some(a), Some(b)) if a != b => {
                    if let Some(i) = self.arrows.iter().position(|(x, y)| *x == a && *y == b) {
                        self.arrows.remove(i);
                    } else {
                        self.arrows.push((a, b));
                    }
                }
                (Some(_), Some(_)) => self.arrows.clear(),
                _ => {}
            }
        }

        if resp.drag_started() {
            if let Some(p) = pointer {
                if let Some(sq) = lay.square_at(p, self.orientation) {
                    if can_move && ours(sq) {
                        self.anim = None;
                        self.selected = Some(sq);
                        let grab = (p - lay.rect(sq, self.orientation).center()) / lay.cell;
                        self.drag = Some(Drag { from: sq, grab });
                    }
                }
            }
        }
        if resp.drag_stopped() {
            if let (Some(d), Some(p)) = (self.drag.take(), pointer) {
                if let Some(to) = lay.square_at(p, self.orientation) {
                    if let Some(mv) = target_move(pos, d.from, to) {
                        out = Some((mv, false));
                    }
                }
                self.selected = None;
            }
        }
        if resp.clicked() {
            if let Some(p) = pointer {
                if let Some(sq) = lay.square_at(p, self.orientation) {
                    match self.selected {
                        Some(sel) if sel == sq => self.selected = None,
                        Some(sel) => {
                            if let Some(mv) = target_move(pos, sel, sq) {
                                out = Some((mv, true));
                                self.selected = None;
                            } else if can_move && ours(sq) {
                                self.selected = Some(sq);
                            } else {
                                self.selected = None;
                            }
                        }
                        None => {
                            if can_move && ours(sq) {
                                self.selected = Some(sq);
                            }
                        }
                    }
                }
            }
        }

        painter.rect_filled(resp.rect, egui::CornerRadius::ZERO, theme.light);
        for sq in Square::ALL {
            let (f, r) = sq_idx(sq);
            if (f + r) % 2 == 0 {
                painter.rect_filled(lay.rect(sq, self.orientation), egui::CornerRadius::ZERO, theme.dark);
            }
        }

        if let Some((a, b)) = self.last {
            let tint = Color32::from_rgba_unmultiplied(155, 199, 0, 105);
            painter.rect_filled(lay.rect(a, self.orientation), egui::CornerRadius::ZERO, tint);
            painter.rect_filled(lay.rect(b, self.orientation), egui::CornerRadius::ZERO, tint);
        }

        if pos.is_check() {
            if let Some(king) = board.king_of(pos.turn()) {
                let c = lay.rect(king, self.orientation).center();
                for i in (1..=7).rev() {
                    let radius = lay.cell * 0.62 * (i as f32 / 7.0);
                    let alpha = (18 + (7 - i) * 20).min(150) as u8;
                    painter.circle_filled(c, radius, Color32::from_rgba_unmultiplied(200, 40, 40, alpha));
                }
            }
        }

        if let Some(sel) = self.selected {
            painter.rect_filled(
                lay.rect(sel, self.orientation),
                egui::CornerRadius::ZERO,
                Color32::from_rgba_unmultiplied(20, 30, 20, 55),
            );
        }

        if let Some(sel) = self.selected {
            for mv in legal_targets(pos, sel) {
                let dest = dest_sq(mv);
                let r = lay.rect(dest, self.orientation);
                if board.piece_at(dest).is_some() {
                    painter.circle_stroke(
                        r.center(),
                        lay.cell * 0.44,
                        Stroke::new(lay.cell * 0.075, Color32::from_rgba_unmultiplied(20, 20, 20, 90)),
                    );
                } else {
                    painter.circle_filled(
                        r.center(),
                        lay.cell * 0.11,
                        Color32::from_rgba_unmultiplied(20, 20, 20, 80),
                    );
                }
            }
        }

        if let (Some(d), Some(h)) = (&self.drag, self.hover) {
            if h != d.from
                && legal_targets(pos, d.from)
                    .iter()
                    .any(|m| dest_sq(*m) == h)
            {
                painter.rect_stroke(
                    lay.rect(h, self.orientation).shrink(2.0),
                    egui::CornerRadius::ZERO,
                    Stroke::new(lay.cell * 0.07, Color32::from_white_alpha(150)),
                    egui::StrokeKind::Inside,
                );
            }
        }

        let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
        let dragging = self.drag.as_ref().map(|d| d.from);
        let landing = self.anim.as_ref().map(|a| a.to_sq);

        for sq in Square::ALL {
            if Some(sq) == dragging {
                continue;
            }
            let Some(piece) = board.piece_at(sq) else { continue };
            if landing == Some(sq) {
                continue;
            }
            let tex = assets.piece(ui.ctx(), set, piece.color == Color::White, piece.role);
            painter.image(tex.id(), lay.rect(sq, self.orientation).shrink(lay.cell * 0.03), uv, Color32::WHITE);
        }

        if let Some(a) = &self.anim {
            let t = (((now - a.t0) / a.dur) as f32).clamp(0.0, 1.0);
            let start = lay.rect(a.from_sq, self.orientation).center();
            let end = lay.rect(a.to_sq, self.orientation).center();
            let p = start + (end - start) * ease_out(t);
            let tex = assets.piece(ui.ctx(), set, a.white, a.role);
            painter.image(
                tex.id(),
                Rect::from_center_size(p, Vec2::splat(lay.cell * 0.94)),
                uv,
                Color32::WHITE,
            );
        }

        if let Some(d) = &self.drag {
            if let Some(piece) = board.piece_at(d.from) {
                let pos = match pointer {
                    Some(p) => p - d.grab * lay.cell,
                    None => lay.rect(d.from, self.orientation).center(),
                };
                let tex = assets.piece(ui.ctx(), set, piece.color == Color::White, piece.role);
                let size = lay.cell * 1.02;
                painter.image(
                    tex.id(),
                    Rect::from_center_size(pos + Vec2::new(2.0, 3.0), Vec2::splat(size)),
                    uv,
                    Color32::from_black_alpha(70),
                );
                painter.image(
                    tex.id(),
                    Rect::from_center_size(pos, Vec2::splat(size)),
                    uv,
                    Color32::WHITE,
                );
            }
        }

        for (a, b) in &self.arrows {
            arrow(&painter, &lay, self.orientation, *a, *b, Color32::from_rgba_unmultiplied(232, 161, 58, 220));
        }
        if let (Some(from), Some(h)) = (self.arrow_from, self.hover) {
            if from != h {
                arrow(&painter, &lay, self.orientation, from, h, Color32::from_rgba_unmultiplied(232, 161, 58, 150));
            }
        }
        if let Some((a, b)) = self.hint {
            arrow(&painter, &lay, self.orientation, a, b, Color32::from_rgba_unmultiplied(129, 182, 76, 230));
        }

        if let Some(b) = badge {
            if now < b.until {
                if let Some(tex) = assets.badge(ui.ctx(), b.cls) {
                    let r = lay.rect(b.sq, self.orientation);
                    let size = lay.cell * 0.42;
                    let x = (r.right() - size * 0.85)
                        .clamp(resp.rect.left(), resp.rect.right() - size);
                    let y = (r.top() - size * 0.15)
                        .clamp(resp.rect.top(), resp.rect.bottom() - size);
                    painter.image(
                        tex.id(),
                        Rect::from_min_size(Pos2::new(x, y), Vec2::splat(size)),
                        uv,
                        Color32::WHITE,
                    );
                }
            }
        }

        if self.show_coords {
            let font = FontId::proportional(lay.cell * 0.17);
            for i in 0..8 {
                let file_char = if self.orientation == Color::White {
                    (b'a' + i as u8) as char
                } else {
                    (b'h' - i as u8) as char
                };
                let rank_char = if self.orientation == Color::White {
                    (b'1' + i as u8) as char
                } else {
                    (b'8' - i as u8) as char
                };
                let bottom = lay.origin + Vec2::new(i as f32 * lay.cell, 7.0 * lay.cell);
                painter.text(
                    bottom + Vec2::new(lay.cell - 3.0, lay.cell - 2.0),
                    Align2::RIGHT_BOTTOM,
                    file_char,
                    font.clone(),
                    if i % 2 == 0 { theme.light } else { theme.dark },
                );
                let left = lay.origin + Vec2::new(0.0, i as f32 * lay.cell);
                painter.text(
                    left + Vec2::new(3.0, 2.0),
                    Align2::LEFT_TOP,
                    rank_char,
                    font.clone(),
                    if i % 2 == 0 { theme.dark } else { theme.light },
                );
            }
        }

        if let Some(a) = &self.anim {
            if now - a.t0 >= a.dur {
                self.anim = None;
            }
        }

        out
    }
}

fn arrow(p: &egui::Painter, lay: &Layout, orientation: Color, a: Square, b: Square, color: Color32) {
    let start = lay.rect(a, orientation).center();
    let end = lay.rect(b, orientation).center();
    let v = end - start;
    let len = v.length().max(0.001);
    let u = v / len;
    let tip = end - u * lay.cell * 0.22;
    p.line_segment([start + u * lay.cell * 0.25, tip], Stroke::new(lay.cell * 0.11, color));
    let head = tip + u * lay.cell * 0.16;
    p.add(egui::Shape::convex_polygon(
        vec![
            head,
            tip + Vec2::new(-u.y, u.x) * lay.cell * 0.18,
            tip + Vec2::new(u.y, -u.x) * lay.cell * 0.18,
        ],
        color,
        Stroke::NONE,
    ));
}