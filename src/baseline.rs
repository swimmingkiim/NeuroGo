use crate::{connect4::Board, policy::PolicyNet, rng::Rng, sparse::SparseNet};

pub trait Agent {
    fn name(&self) -> &str;
    fn choose(&mut self, b: &Board, rng: &mut Rng) -> usize;
}
pub struct RandomAgent;
impl Agent for RandomAgent {
    fn name(&self) -> &str {
        "random"
    }
    fn choose(&mut self, b: &Board, rng: &mut Rng) -> usize {
        let m = b.legal_moves();
        m[rng.usize(m.len())]
    }
}

pub struct HeuristicAgent;
impl Agent for HeuristicAgent {
    fn name(&self) -> &str {
        "heuristic"
    }
    fn choose(&mut self, b: &Board, rng: &mut Rng) -> usize {
        let legal = b.legal_moves();
        for &c in &legal {
            let mut n = *b;
            n.play(c).unwrap();
            if n.winner() == b.turn {
                return c;
            }
        }
        for &c in &legal {
            let mut n = *b;
            n.turn = -n.turn;
            n.play(c).unwrap();
            if n.winner() == -b.turn {
                return c;
            }
        }
        let mut ranked: Vec<_> = legal
            .into_iter()
            .map(|c| ((c as i32 - 3).abs(), rng.next_u64(), c))
            .collect();
        ranked.sort();
        ranked[0].2
    }
}

pub struct SearchAgent {
    pub depth: u8,
}
impl SearchAgent {
    fn negamax(b: &Board, depth: u8, mut alpha: i32, beta: i32) -> i32 {
        let w = b.winner();
        if w != 0 {
            return if w == b.turn { 10000 } else { -10000 };
        }
        if depth == 0 || b.terminal() {
            return 0;
        }
        let mut best = -20000;
        for c in b.legal_moves() {
            let mut n = *b;
            n.play(c).unwrap();
            let v = -Self::negamax(&n, depth - 1, -beta, -alpha);
            best = best.max(v);
            alpha = alpha.max(v);
            if alpha >= beta {
                break;
            }
        }
        best
    }
}
impl Agent for SearchAgent {
    fn name(&self) -> &str {
        "search"
    }
    fn choose(&mut self, b: &Board, rng: &mut Rng) -> usize {
        let mut scored = Vec::new();
        for c in b.legal_moves() {
            let mut n = *b;
            n.play(c).unwrap();
            scored.push((
                -Self::negamax(&n, self.depth.saturating_sub(1), -20000, 20000),
                rng.next_u64(),
                c,
            ));
        }
        scored.sort_by(|a, b| b.cmp(a));
        scored[0].2
    }
}

pub struct SparseAgent<'a> {
    pub net: &'a SparseNet,
}

pub struct SparseSearchAgent<'a> {
    pub net: &'a SparseNet,
    pub depth: u8,
}
impl SparseSearchAgent<'_> {
    fn negamax(&self, b: &Board, depth: u8, mut alpha: f32, beta: f32) -> f32 {
        if b.winner() != 0 {
            return -1.0;
        }
        if b.terminal() {
            return 0.0;
        }
        if depth == 0 {
            return self.net.predict(&b.features());
        }
        let mut best: f32 = -2.0;
        for col in b.legal_moves() {
            let mut next = *b;
            next.play(col).unwrap();
            let value = -self.negamax(&next, depth - 1, -beta, -alpha);
            best = best.max(value);
            alpha = alpha.max(value);
            if alpha >= beta {
                break;
            }
        }
        best
    }
    pub fn move_value(&self, b: &Board, col: usize) -> f32 {
        let mut next = *b;
        next.play(col).unwrap();
        -self.negamax(&next, self.depth.saturating_sub(1), -2.0, 2.0)
    }
}
impl Agent for SparseSearchAgent<'_> {
    fn name(&self) -> &str {
        "sparse-search"
    }
    fn choose(&mut self, b: &Board, rng: &mut Rng) -> usize {
        let mut scored = Vec::new();
        for col in b.legal_moves() {
            let value = self.move_value(b, col);
            scored.push((value, rng.next_u64(), col));
        }
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap().then_with(|| b.1.cmp(&a.1)));
        scored[0].2
    }
}

pub struct HybridAgent<'a> {
    pub value: &'a SparseNet,
    pub policy: &'a PolicyNet,
    pub depth: u8,
    pub policy_weight: f32,
}
impl Agent for HybridAgent<'_> {
    fn name(&self) -> &str {
        "hybrid"
    }
    fn choose(&mut self, b: &Board, rng: &mut Rng) -> usize {
        let policy_scores = self.policy.scores(b);
        let search = SparseSearchAgent {
            net: self.value,
            depth: self.depth,
        };
        let mut scored = Vec::new();
        for col in b.legal_moves() {
            scored.push((
                search.move_value(b, col) + self.policy_weight * policy_scores[col],
                rng.next_u64(),
                col,
            ));
        }
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap().then_with(|| b.1.cmp(&a.1)));
        scored[0].2
    }
}
impl Agent for SparseAgent<'_> {
    fn name(&self) -> &str {
        "sparse"
    }
    fn choose(&mut self, b: &Board, rng: &mut Rng) -> usize {
        let mut s = Vec::new();
        for c in b.legal_moves() {
            let mut n = *b;
            n.play(c).unwrap();
            s.push((-self.net.predict(&n.features()), rng.next_u64(), c));
        }
        s.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap().then_with(|| b.1.cmp(&a.1)));
        s[0].2
    }
}
