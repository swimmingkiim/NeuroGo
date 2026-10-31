use crate::{
    connect4::{Board, COLS, FEATURES},
    dataset::PolicySample,
    rng::Rng,
};
use std::{
    fs::File,
    io::{BufRead, BufReader, BufWriter, Write},
};

#[derive(Clone)]
pub struct PolicyNet {
    pub weights: Vec<f32>,
}

impl PolicyNet {
    pub fn new() -> Self {
        Self {
            weights: vec![0.0; COLS * FEATURES],
        }
    }
    pub fn scores(&self, board: &Board) -> [f32; COLS] {
        let x = board.features();
        let mut out = [f32::NEG_INFINITY; COLS];
        for col in board.legal_moves() {
            out[col] = (0..FEATURES)
                .filter(|&i| x[i] != 0.0)
                .map(|i| self.weights[col * FEATURES + i] * x[i])
                .sum();
        }
        out
    }
    pub fn train(&mut self, data: &[PolicySample], epochs: usize, seed: u64, lr: f32) -> f32 {
        let mut rng = Rng::new(seed);
        let mut order: Vec<_> = (0..data.len()).collect();
        let mut loss = 0.0;
        for _ in 0..epochs {
            rng.shuffle(&mut order);
            loss = 0.0;
            for &idx in &order {
                let sample = &data[idx];
                let x = sample.board.features();
                let scores = self.scores(&sample.board);
                let legal = sample.board.legal_moves();
                let max = legal
                    .iter()
                    .map(|&c| scores[c])
                    .fold(f32::NEG_INFINITY, f32::max);
                let denom: f32 = legal.iter().map(|&c| (scores[c] - max).exp()).sum();
                for &col in &legal {
                    if sample.targets[col] > 0.0 {
                        loss += -sample.targets[col] * (scores[col] - max - denom.ln());
                    }
                }
                for &col in &legal {
                    let p = (scores[col] - max).exp() / denom;
                    let grad = p - sample.targets[col];
                    for i in 0..FEATURES {
                        if x[i] != 0.0 {
                            self.weights[col * FEATURES + i] -= lr * grad * x[i];
                        }
                    }
                }
            }
            loss /= data.len().max(1) as f32;
        }
        loss
    }
    pub fn save(&self, path: &str) -> Result<(), String> {
        let mut w = BufWriter::new(File::create(path).map_err(|e| e.to_string())?);
        writeln!(w, "NFP1 {}", FEATURES).map_err(|e| e.to_string())?;
        for chunk in self.weights.chunks(FEATURES) {
            for v in chunk {
                write!(w, "{v} ").map_err(|e| e.to_string())?;
            }
            writeln!(w).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    pub fn load(path: &str) -> Result<Self, String> {
        let mut lines = BufReader::new(File::open(path).map_err(|e| e.to_string())?).lines();
        let h = lines
            .next()
            .ok_or("empty policy")?
            .map_err(|e| e.to_string())?;
        if h != format!("NFP1 {FEATURES}") {
            return Err("incompatible policy".into());
        }
        let mut weights = Vec::new();
        for l in lines {
            for v in l.map_err(|e| e.to_string())?.split_whitespace() {
                weights.push(v.parse().map_err(|_| "bad policy weight")?);
            }
        }
        if weights.len() != COLS * FEATURES {
            return Err("bad policy size".into());
        }
        Ok(Self { weights })
    }
}
