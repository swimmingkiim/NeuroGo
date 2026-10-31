use crate::rng::Rng;
use std::{
    collections::VecDeque,
    fs::File,
    io::{BufRead, BufReader, BufWriter, Write},
};

#[derive(Clone, Debug)]
pub struct GoBoard {
    pub size: usize,
    pub cells: Vec<i8>,
    pub turn: i8,
    pub ko: Option<usize>,
    pub passes: u8,
    pub moves: usize,
}

impl GoBoard {
    pub fn new(size: usize) -> Result<Self, String> {
        if !(5..=19).contains(&size) {
            return Err("Go size must be 5..19".into());
        }
        Ok(Self {
            size,
            cells: vec![0; size * size],
            turn: 1,
            ko: None,
            passes: 0,
            moves: 0,
        })
    }
    pub fn pass_action(&self) -> usize {
        self.cells.len()
    }
    fn neighbors(&self, p: usize) -> Vec<usize> {
        let r = p / self.size;
        let c = p % self.size;
        let mut n = Vec::with_capacity(4);
        if r > 0 {
            n.push(p - self.size)
        }
        if r + 1 < self.size {
            n.push(p + self.size)
        }
        if c > 0 {
            n.push(p - 1)
        }
        if c + 1 < self.size {
            n.push(p + 1)
        }
        n
    }
    fn group(&self, start: usize) -> (Vec<usize>, usize) {
        let color = self.cells[start];
        let mut seen = vec![false; self.cells.len()];
        let mut q = VecDeque::new();
        let mut stones = Vec::new();
        let mut liberties = vec![false; self.cells.len()];
        seen[start] = true;
        q.push_back(start);
        while let Some(p) = q.pop_front() {
            stones.push(p);
            for n in self.neighbors(p) {
                if self.cells[n] == 0 {
                    liberties[n] = true
                } else if self.cells[n] == color && !seen[n] {
                    seen[n] = true;
                    q.push_back(n);
                }
            }
        }
        (stones, liberties.into_iter().filter(|x| *x).count())
    }
    pub fn play(&mut self, action: usize) -> Result<(), String> {
        if action == self.pass_action() {
            self.turn = -self.turn;
            self.ko = None;
            self.passes += 1;
            self.moves += 1;
            return Ok(());
        }
        if action >= self.cells.len() || self.cells[action] != 0 {
            return Err("illegal occupied Go move".into());
        }
        if self.ko == Some(action) {
            return Err("illegal ko recapture".into());
        }
        let before = self.clone();
        self.cells[action] = self.turn;
        let mut captured = Vec::new();
        for n in self.neighbors(action) {
            if self.cells[n] == -self.turn {
                let (g, l) = self.group(n);
                if l == 0 {
                    for p in g {
                        self.cells[p] = 0;
                        captured.push(p);
                    }
                }
            }
        }
        if self.group(action).1 == 0 {
            *self = before;
            return Err("illegal suicide".into());
        }
        let own_group = self.group(action).0;
        self.ko = if captured.len() == 1 && own_group.len() == 1 {
            Some(captured[0])
        } else {
            None
        };
        self.turn = -self.turn;
        self.passes = 0;
        self.moves += 1;
        Ok(())
    }
    pub fn legal_moves(&self) -> Vec<usize> {
        let mut out = Vec::new();
        for p in 0..self.cells.len() {
            if self.cells[p] == 0 && self.ko != Some(p) {
                let mut b = self.clone();
                if b.play(p).is_ok() {
                    out.push(p)
                }
            }
        }
        out.push(self.pass_action());
        out
    }
    pub fn terminal(&self) -> bool {
        self.passes >= 2 || self.moves >= self.cells.len() * 3
    }
    pub fn score_black(&self, komi: f32) -> f32 {
        let mut score = -komi;
        let mut seen = vec![false; self.cells.len()];
        for p in 0..self.cells.len() {
            match self.cells[p] {
                1 => score += 1.0,
                -1 => score -= 1.0,
                _ if !seen[p] => {
                    let mut q = VecDeque::new();
                    let mut area = Vec::new();
                    let (mut black, mut white) = (false, false);
                    seen[p] = true;
                    q.push_back(p);
                    while let Some(x) = q.pop_front() {
                        area.push(x);
                        for n in self.neighbors(x) {
                            match self.cells[n] {
                                1 => black = true,
                                -1 => white = true,
                                _ if !seen[n] => {
                                    seen[n] = true;
                                    q.push_back(n)
                                }
                                _ => {}
                            }
                        }
                    }
                    if black && !white {
                        score += area.len() as f32
                    } else if white && !black {
                        score -= area.len() as f32
                    }
                }
                _ => {}
            }
        }
        score
    }
    pub fn features(&self) -> Vec<f32> {
        let n = self.cells.len();
        let mut x = vec![0.0; n * 2 + 1];
        for i in 0..n {
            if self.cells[i] == self.turn {
                x[i] = 1.0
            } else if self.cells[i] == -self.turn {
                x[n + i] = 1.0
            }
        }
        x[n * 2] = 1.0;
        x
    }
}

#[derive(Clone)]
pub struct GoGame {
    pub size: usize,
    pub moves: Vec<usize>,
    pub winner: i8,
}
pub fn generate(path: &str, size: usize, games: usize, seed: u64) -> Result<(), String> {
    let mut rng = Rng::new(seed);
    let mut w = BufWriter::new(File::create(path).map_err(|e| e.to_string())?);
    for g in 0..games {
        let mut b = GoBoard::new(size)?;
        let mut seq = Vec::new();
        while !b.terminal() {
            let legal = b.legal_moves();
            let action = if rng.usize(10) < 3 {
                legal[rng.usize(legal.len())]
            } else {
                heuristic_move(&b, &legal, &mut rng)
            };
            b.play(action)?;
            seq.push(action);
        }
        let winner = if b.score_black(6.5) > 0.0 { 1 } else { -1 };
        let moves = seq
            .iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>()
            .join(" ");
        writeln!(w, "{size},{winner},{moves}").map_err(|e| e.to_string())?;
        if g % 100 == 99 {
            w.flush().map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
pub fn heuristic_move(b: &GoBoard, legal: &[usize], rng: &mut Rng) -> usize {
    let center = (b.size as f32 - 1.0) / 2.0;
    let mut scored = Vec::new();
    for &a in legal {
        if a == b.pass_action() {
            scored.push((-1000.0, rng.next_u64(), a));
            continue;
        }
        let before = b.cells.iter().filter(|&&x| x == -b.turn).count();
        let mut n = b.clone();
        n.play(a).unwrap();
        let after = n.cells.iter().filter(|&&x| x == b.turn).count();
        let captured = (before - after) as f32;
        let r = (a / b.size) as f32;
        let c = (a % b.size) as f32;
        let central = -((r - center).abs() + (c - center).abs()) * 0.01;
        scored.push((captured * 10.0 + central, rng.next_u64(), a));
    }
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap().then_with(|| b.1.cmp(&a.1)));
    scored[0].2
}
pub fn load_games(path: &str) -> Result<Vec<GoGame>, String> {
    let mut out = Vec::new();
    for line in BufReader::new(File::open(path).map_err(|e| e.to_string())?).lines() {
        let line = line.map_err(|e| e.to_string())?;
        let mut p = line.splitn(3, ',');
        let size = p
            .next()
            .ok_or("bad Go row")?
            .parse()
            .map_err(|_| "bad size")?;
        let winner = p
            .next()
            .ok_or("bad Go row")?
            .parse()
            .map_err(|_| "bad winner")?;
        let moves = p
            .next()
            .unwrap_or("")
            .split_whitespace()
            .map(|x| x.parse().map_err(|_| "bad move"))
            .collect::<Result<_, _>>()?;
        out.push(GoGame {
            size,
            moves,
            winner,
        });
    }
    Ok(out)
}

#[derive(Clone)]
pub struct GoModel {
    pub size: usize,
    pub value: Vec<f32>,
    pub policy: Vec<f32>,
}
impl GoModel {
    pub fn new(size: usize) -> Self {
        let f = size * size * 2 + 1;
        Self {
            size,
            value: vec![0.0; f],
            policy: vec![0.0; (size * size + 1) * f],
        }
    }
    fn dot(w: &[f32], x: &[f32]) -> f32 {
        w.iter().zip(x).map(|(a, b)| a * b).sum()
    }
    pub fn value(&self, b: &GoBoard) -> f32 {
        Self::dot(&self.value, &b.features()).tanh()
    }
    pub fn choose(&self, b: &GoBoard, rng: &mut Rng) -> usize {
        let x = b.features();
        let f = x.len();
        let mut scored = Vec::new();
        for a in b.legal_moves() {
            let mut next = b.clone();
            next.play(a).unwrap();
            let s = Self::dot(&self.policy[a * f..(a + 1) * f], &x) - self.value(&next);
            scored.push((s, rng.next_u64(), a));
        }
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap().then_with(|| b.1.cmp(&a.1)));
        scored[0].2
    }
    pub fn train(&mut self, games: &[GoGame], epochs: usize, seed: u64) -> Result<f32, String> {
        let mut rng = Rng::new(seed);
        let f = self.value.len();
        let mut loss = 0.0;
        for _ in 0..epochs {
            let mut order: Vec<_> = (0..games.len()).collect();
            rng.shuffle(&mut order);
            loss = 0.0;
            let mut count = 0;
            for i in order {
                let game = &games[i];
                if game.size != self.size {
                    continue;
                }
                let mut b = GoBoard::new(self.size)?;
                for &action in &game.moves {
                    let x = b.features();
                    let target = if game.winner == b.turn { 1.0 } else { -1.0 };
                    let pred = self.value(&b);
                    let err = target - pred;
                    let legal = b.legal_moves();
                    let alternatives: Vec<_> = legal.into_iter().filter(|&a| a != action).collect();
                    let negative = if alternatives.is_empty() {
                        None
                    } else {
                        Some(alternatives[rng.usize(alternatives.len())])
                    };
                    for j in 0..f {
                        if x[j] != 0.0 {
                            self.value[j] += 0.01 * err * x[j];
                            self.policy[action * f + j] += 0.002 * x[j];
                            if let Some(other) = negative {
                                self.policy[other * f + j] -= 0.002 * x[j];
                            }
                        }
                    }
                    loss += err * err;
                    count += 1;
                    b.play(action)?;
                }
            }
            loss /= count.max(1) as f32;
        }
        Ok(loss)
    }
    pub fn save(&self, path: &str) -> Result<(), String> {
        let mut w = BufWriter::new(File::create(path).map_err(|e| e.to_string())?);
        writeln!(w, "NGO1 {}", self.size).map_err(|e| e.to_string())?;
        for v in self.value.iter().chain(self.policy.iter()) {
            writeln!(w, "{v}").map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    pub fn load(path: &str) -> Result<Self, String> {
        let mut lines = BufReader::new(File::open(path).map_err(|e| e.to_string())?).lines();
        let h = lines
            .next()
            .ok_or("empty Go model")?
            .map_err(|e| e.to_string())?;
        let size = h
            .split_whitespace()
            .nth(1)
            .ok_or("bad Go model")?
            .parse()
            .map_err(|_| "bad size")?;
        let mut m = Self::new(size);
        let vals = lines
            .map(|l| {
                l.map_err(|e| e.to_string())?
                    .parse()
                    .map_err(|_| "bad weight".to_string())
            })
            .collect::<Result<Vec<f32>, _>>()?;
        let f = m.value.len();
        if vals.len() != f + m.policy.len() {
            return Err("bad Go model length".into());
        }
        m.value.copy_from_slice(&vals[..f]);
        m.policy.copy_from_slice(&vals[f..]);
        Ok(m)
    }
}

pub fn benchmark(
    model: &GoModel,
    games: usize,
    seed: u64,
    heuristic: bool,
) -> Result<(usize, usize), String> {
    let mut rng = Rng::new(seed);
    let mut wins = 0;
    for g in 0..games {
        let model_black = g % 2 == 0;
        let mut board = GoBoard::new(model.size)?;
        while !board.terminal() {
            let model_turn = (board.turn == 1) == model_black;
            let action = if model_turn {
                model.choose(&board, &mut rng)
            } else {
                let legal = board.legal_moves();
                if heuristic {
                    heuristic_move(&board, &legal, &mut rng)
                } else {
                    legal[rng.usize(legal.len())]
                }
            };
            board.play(action)?;
        }
        let black_win = board.score_black(6.5) > 0.0;
        if black_win == model_black {
            wins += 1;
        }
    }
    Ok((wins, games - wins))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capture_and_suicide() {
        let mut b = GoBoard::new(5).unwrap();
        for a in [1, 0, 5, 6, 25, 2, 25, 10] {
            b.play(a).unwrap();
        }
        assert_eq!(b.cells[0], 0);
    }
    #[test]
    fn empty_score_is_komi() {
        let b = GoBoard::new(9).unwrap();
        assert_eq!(b.score_black(6.5), -6.5);
    }
}
