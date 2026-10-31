use crate::{baseline::Agent, connect4::Board, rng::Rng};
use std::time::{Duration, Instant};
#[derive(Debug)]
pub struct ArenaResult {
    pub games: usize,
    pub a_wins: usize,
    pub b_wins: usize,
    pub draws: usize,
    pub elapsed: Duration,
}
impl ArenaResult {
    pub fn score(&self) -> f64 {
        (self.a_wins as f64 + 0.5 * self.draws as f64) / self.games as f64
    }
    pub fn elo(&self) -> f64 {
        let p = self.score().clamp(0.001, 0.999);
        400.0 * (p / (1.0 - p)).log10()
    }
}
pub fn play(a: &mut dyn Agent, b: &mut dyn Agent, games: usize, seed: u64) -> ArenaResult {
    let start = Instant::now();
    let mut rng = Rng::new(seed);
    let (mut aw, mut bw, mut d) = (0, 0, 0);
    for g in 0..games {
        let a_first = g % 2 == 0;
        let mut board = Board::default();
        while !board.terminal() {
            let a_turn = (board.turn == 1) == a_first;
            let c = if a_turn {
                a.choose(&board, &mut rng)
            } else {
                b.choose(&board, &mut rng)
            };
            board.play(c).unwrap();
        }
        match board.winner() {
            0 => d += 1,
            1 if a_first => aw += 1,
            -1 if !a_first => aw += 1,
            _ => bw += 1,
        }
    }
    ArenaResult {
        games,
        a_wins: aw,
        b_wins: bw,
        draws: d,
        elapsed: start.elapsed(),
    }
}
