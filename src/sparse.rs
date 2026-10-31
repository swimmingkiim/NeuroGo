use crate::{connect4::FEATURES, dataset::Sample, rng::Rng};
use std::{
    cmp::Ordering,
    fs::File,
    io::{BufRead, BufReader, BufWriter, Write},
};

#[derive(Clone, Debug)]
pub struct Edge {
    pub input: u16,
    pub weight: f32,
    pub strength: f32,
    pub last_active: u64,
}
#[derive(Clone, Debug)]
pub struct Node {
    pub bias: f32,
    pub output: f32,
    pub utility: f32,
    pub dormant: bool,
    pub edges: Vec<Edge>,
}
#[derive(Clone, Debug)]
pub struct Config {
    pub max_nodes: usize,
    pub max_edges_per_node: usize,
    pub top_k: usize,
    pub lr: f32,
    pub novelty: f32,
    pub error_threshold: f32,
    pub prune_strength: f32,
    pub dormancy_steps: u64,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            max_nodes: 128,
            max_edges_per_node: 8,
            top_k: 8,
            lr: 0.025,
            novelty: 0.20,
            error_threshold: 0.55,
            prune_strength: 0.03,
            dormancy_steps: 2000,
        }
    }
}
#[derive(Clone, Debug)]
pub struct SparseNet {
    pub cfg: Config,
    pub nodes: Vec<Node>,
    pub step: u64,
}

impl SparseNet {
    pub fn new(cfg: Config, seed: u64) -> Self {
        let mut rng = Rng::new(seed);
        let mut nodes = Vec::new();
        for _ in 0..cfg.top_k.min(cfg.max_nodes) {
            let input = rng.usize(FEATURES) as u16;
            nodes.push(Node {
                bias: 0.0,
                output: rng.f32() * 0.2 - 0.1,
                utility: 0.1,
                dormant: false,
                edges: vec![Edge {
                    input,
                    weight: rng.f32() * 0.2 - 0.1,
                    strength: 0.1,
                    last_active: 0,
                }],
            });
        }
        Self {
            cfg,
            nodes,
            step: 0,
        }
    }
    fn activations(&self, x: &[f32; FEATURES]) -> Vec<(usize, f32)> {
        let mut a: Vec<_> = self
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| !n.dormant)
            .map(|(i, n)| {
                (
                    i,
                    (n.bias
                        + n.edges
                            .iter()
                            .map(|e| e.weight * x[e.input as usize])
                            .sum::<f32>())
                    .tanh(),
                )
            })
            .collect();
        a.sort_by(|a, b| b.1.abs().partial_cmp(&a.1.abs()).unwrap_or(Ordering::Equal));
        a.truncate(self.cfg.top_k);
        a
    }
    pub fn predict(&self, x: &[f32; FEATURES]) -> f32 {
        self.activations(x)
            .iter()
            .map(|(i, a)| self.nodes[*i].output * a)
            .sum::<f32>()
            .tanh()
    }
    pub fn train_sample(&mut self, x: &[f32; FEATURES], target: f32, rng: &mut Rng) -> f32 {
        self.step += 1;
        let acts = self.activations(x);
        let pred = acts
            .iter()
            .map(|(i, a)| self.nodes[*i].output * a)
            .sum::<f32>()
            .tanh();
        let err = target - pred;
        let active_inputs: Vec<usize> = x
            .iter()
            .enumerate()
            .filter(|(_, v)| **v != 0.0)
            .map(|(i, _)| i)
            .collect();
        for &(i, a) in &acts {
            let n = &mut self.nodes[i];
            let contribution = (err * n.output * a).abs();
            n.output += self.cfg.lr * err * a;
            n.utility = n.utility * 0.995 + contribution * 0.005;
            for e in &mut n.edges {
                if x[e.input as usize] != 0.0 {
                    e.weight += self.cfg.lr * err * n.output * (1.0 - a * a) * x[e.input as usize];
                    e.strength = e.strength * 0.99 + (a.abs() * err.abs()) * 0.01;
                    e.last_active = self.step;
                }
            }
            // A selected node grows toward a co-active feature it does not yet
            // represent. Error gates growth; the cap preserves sparse memory.
            if err.abs() > self.cfg.error_threshold && n.edges.len() < self.cfg.max_edges_per_node {
                let missing: Vec<usize> = active_inputs
                    .iter()
                    .copied()
                    .filter(|j| !n.edges.iter().any(|e| e.input as usize == *j))
                    .collect();
                if !missing.is_empty() {
                    let j = missing[rng.usize(missing.len())];
                    n.edges.push(Edge {
                        input: j as u16,
                        weight: err.signum() * 0.05,
                        strength: err.abs() * a.abs().max(0.1),
                        last_active: self.step,
                    });
                }
            }
        }
        let covered = active_inputs
            .iter()
            .filter(|&&j| {
                self.nodes
                    .iter()
                    .any(|n| n.edges.iter().any(|e| e.input as usize == j))
            })
            .count();
        let novelty = 1.0 - covered as f32 / active_inputs.len().max(1) as f32;
        if err.abs() > self.cfg.error_threshold
            && novelty > self.cfg.novelty
            && self.nodes.len() < self.cfg.max_nodes
        {
            let j = active_inputs[rng.usize(active_inputs.len())];
            self.nodes.push(Node {
                bias: 0.0,
                output: err.signum() * 0.1,
                utility: err.abs() * 0.1,
                dormant: false,
                edges: vec![Edge {
                    input: j as u16,
                    weight: 0.1,
                    strength: err.abs() * 0.1,
                    last_active: self.step,
                }],
            });
        }
        if self.step % 256 == 0 {
            self.maintain();
        }
        err * err
    }
    fn maintain(&mut self) {
        for n in &mut self.nodes {
            n.edges.retain(|e| {
                e.strength >= self.cfg.prune_strength
                    || self.step - e.last_active < self.cfg.dormancy_steps
            });
            n.dormant = n.edges.is_empty() || n.utility < 0.0001;
        }
        self.nodes.retain(|n| !(n.dormant && n.utility < 0.00001));
    }
    pub fn train(&mut self, data: &[Sample], epochs: usize, seed: u64) -> f32 {
        let mut rng = Rng::new(seed);
        let mut loss = 0.0;
        for _ in 0..epochs {
            loss = self.train_epoch(data, &mut rng);
        }
        loss
    }
    pub fn train_epoch(&mut self, data: &[Sample], rng: &mut Rng) -> f32 {
        let mut order: Vec<usize> = (0..data.len()).collect();
        rng.shuffle(&mut order);
        let mut loss = 0.0;
        for i in order {
            let s = &data[i];
            loss += self.train_sample(&s.board.features(), s.value, rng);
        }
        loss / data.len().max(1) as f32
    }
    pub fn mse(&self, data: &[Sample]) -> f32 {
        data.iter()
            .map(|s| {
                let e = s.value - self.predict(&s.board.features());
                e * e
            })
            .sum::<f32>()
            / data.len().max(1) as f32
    }
    pub fn edge_count(&self) -> usize {
        self.nodes.iter().map(|n| n.edges.len()).sum()
    }
    pub fn save(&self, path: &str) -> Result<(), String> {
        let mut w = BufWriter::new(File::create(path).map_err(|e| e.to_string())?);
        writeln!(
            w,
            "NSN2 {} {} {} {} {} {} {} {} {}",
            self.step,
            self.cfg.max_nodes,
            self.cfg.max_edges_per_node,
            self.cfg.top_k,
            self.cfg.lr,
            self.cfg.novelty,
            self.cfg.error_threshold,
            self.cfg.prune_strength,
            self.cfg.dormancy_steps
        )
        .map_err(|e| e.to_string())?;
        for n in &self.nodes {
            write!(
                w,
                "N {} {} {} {}",
                n.bias, n.output, n.utility, n.dormant as u8
            )
            .map_err(|e| e.to_string())?;
            for e in &n.edges {
                write!(
                    w,
                    " {}:{}:{}:{}",
                    e.input, e.weight, e.strength, e.last_active
                )
                .map_err(|e| e.to_string())?;
            }
            writeln!(w).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    pub fn load(path: &str) -> Result<Self, String> {
        let mut lines = BufReader::new(File::open(path).map_err(|e| e.to_string())?).lines();
        let h = lines
            .next()
            .ok_or("empty checkpoint")?
            .map_err(|e| e.to_string())?;
        let p: Vec<_> = h.split_whitespace().collect();
        if (p[0] != "NSN1" || p.len() != 9) && (p[0] != "NSN2" || p.len() != 10) {
            return Err("unsupported checkpoint".into());
        }
        let v2 = p[0] == "NSN2";
        let offset = if v2 { 1 } else { 0 };
        let parse = |i: usize| {
            p[i].parse::<f32>()
                .map_err(|_| "bad checkpoint".to_string())
        };
        let cfg = Config {
            max_nodes: p[2].parse().map_err(|_| "bad max_nodes")?,
            max_edges_per_node: if v2 {
                p[3].parse().map_err(|_| "bad max_edges")?
            } else {
                1
            },
            top_k: p[3 + offset].parse().map_err(|_| "bad top_k")?,
            lr: parse(4 + offset)?,
            novelty: parse(5 + offset)?,
            error_threshold: parse(6 + offset)?,
            prune_strength: parse(7 + offset)?,
            dormancy_steps: p[8 + offset].parse().map_err(|_| "bad dormancy")?,
        };
        let mut nodes = Vec::new();
        for l in lines {
            let l = l.map_err(|e| e.to_string())?;
            let q: Vec<_> = l.split_whitespace().collect();
            if q.len() < 5 {
                continue;
            }
            let mut edges = Vec::new();
            for z in &q[5..] {
                let v: Vec<_> = z.split(':').collect();
                edges.push(Edge {
                    input: v[0].parse().map_err(|_| "bad edge")?,
                    weight: v[1].parse().map_err(|_| "bad edge")?,
                    strength: v[2].parse().map_err(|_| "bad edge")?,
                    last_active: v[3].parse().map_err(|_| "bad edge")?,
                });
            }
            nodes.push(Node {
                bias: q[1].parse().map_err(|_| "bad node")?,
                output: q[2].parse().map_err(|_| "bad node")?,
                utility: q[3].parse().map_err(|_| "bad node")?,
                dormant: q[4] == "1",
                edges,
            });
        }
        Ok(Self {
            cfg,
            nodes,
            step: p[1].parse().map_err(|_| "bad step")?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connect4::Board;
    #[test]
    fn learns_and_roundtrips() {
        let mut n = SparseNet::new(Config::default(), 1);
        let x = Board::default().features();
        let before = n.predict(&x);
        let mut r = Rng::new(2);
        for _ in 0..100 {
            n.train_sample(&x, 1.0, &mut r);
        }
        assert!(n.predict(&x) > before);
        assert!(n.edge_count() > n.nodes.len());
        let p = std::env::temp_dir().join("neurofour-test.ckpt");
        n.save(p.to_str().unwrap()).unwrap();
        let m = SparseNet::load(p.to_str().unwrap()).unwrap();
        assert!((n.predict(&x) - m.predict(&x)).abs() < 1e-5);
    }
}
