use neurofour::{
    arena,
    baseline::{
        Agent, HeuristicAgent, HybridAgent, RandomAgent, SearchAgent, SparseAgent,
        SparseSearchAgent,
    },
    dataset,
    policy::PolicyNet,
    rng::Rng,
    sparse::{Config, SparseNet},
};
use std::{collections::BTreeSet, fs::File, io::Write};
use std::{env, process, time::Instant};

fn usage() {
    eprintln!("neurofour <command>\n  generate <file> [games] [seed]\n  import-uci <uci.csv> <native.csv>\n  import-pons <pons.json> <native.csv>\n  import-solver <solver-output> <native.csv>\n  import-zenodo <optimal.csv> <native.csv> [position_limit=100000]\n  merge <output.csv> <input.csv>...\n  mine-hard <checkpoint> <sequences.txt> [games=500] [seed=42] [min_ply=18]\n  train <data> <checkpoint> [max_epochs] [seed] [max_nodes] [top_k] [max_edges_per_node] [patience]\n  train-policy <zenodo.csv> <policy> [positions=100000] [epochs=3] [seed=42] [hard_analysis] [hard_repeat=1]\n  go-generate <size> <games> <file> [seed=42]\n  go-train <data> <model> [epochs=2] [seed=42]\n  go-benchmark <model> [games=20] [seed=42]\n  eval <checkpoint> [opponent=random|heuristic|search] [games] [seed] [depth]\n  benchmark <checkpoint> [games] [seed]\n  benchmark-search <checkpoint> [model_depth=2] [games=100] [seed=42]\n  benchmark-hybrid <value> <policy> [depth=2] [policy_weight=.1] [games=100] [seed=42]\n  inspect <checkpoint>");
}
fn val<T: std::str::FromStr>(xs: &[String], i: usize, default: T) -> T {
    xs.get(i).and_then(|s| s.parse().ok()).unwrap_or(default)
}
fn opponent(name: &str, depth: u8) -> Box<dyn Agent> {
    match name {
        "random" => Box::new(RandomAgent),
        "heuristic" => Box::new(HeuristicAgent),
        "search" => Box::new(SearchAgent { depth }),
        _ => {
            eprintln!("unknown opponent: {name}");
            process::exit(2)
        }
    }
}
fn memory_kb() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmHWM:"))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|v| v.parse().ok())
        })
        .unwrap_or(0)
}

fn run() -> Result<(), String> {
    let a: Vec<String> = env::args().collect();
    match a.get(1).map(String::as_str) {
        Some("generate") => {
            let path = a.get(2).ok_or("missing output file")?;
            let games = val(&a, 3, 1000);
            let seed = val(&a, 4, 42);
            let t = Instant::now();
            let n = dataset::generate(path, games, seed)?;
            println!(
                "generated games={games} samples={n} seed={seed} file={path} time_ms={}",
                t.elapsed().as_millis()
            );
        }
        Some("import-uci") => {
            let input = a.get(2).ok_or("missing UCI CSV")?;
            let output = a.get(3).ok_or("missing output file")?;
            let n = dataset::import_uci(input, output)?;
            println!("imported source=uci samples={n} file={output}");
        }
        Some("import-pons") => {
            let input = a.get(2).ok_or("missing Pons JSON")?;
            let output = a.get(3).ok_or("missing output file")?;
            let n = dataset::import_pons_value(input, output)?;
            println!("imported source=pons-value samples={n} file={output}");
        }
        Some("import-solver") => {
            let input = a.get(2).ok_or("missing solver output")?;
            let output = a.get(3).ok_or("missing output")?;
            let n = dataset::import_solver_values(input, output)?;
            println!("imported source=exact-solver samples={n} file={output}");
        }
        Some("import-zenodo") => {
            let input = a.get(2).ok_or("missing Zenodo CSV")?;
            let output = a.get(3).ok_or("missing output file")?;
            let limit = val(&a, 4, 100_000usize);
            let s = dataset::import_zenodo_policy(input, output, limit)?;
            println!(
                "imported source=zenodo positions={} samples={} skipped={} file={output}",
                s.positions, s.samples, s.skipped
            );
        }
        Some("merge") => {
            let output = a.get(2).ok_or("missing output file")?;
            if a.len() < 4 {
                return Err("merge requires at least one input".into());
            }
            let mut merged = Vec::new();
            for input in &a[3..] {
                merged.extend(dataset::load(input)?);
            }
            dataset::save(output, &merged)?;
            println!(
                "merged inputs={} samples={} file={output}",
                a.len() - 3,
                merged.len()
            );
        }
        Some("mine-hard") => {
            let net = SparseNet::load(a.get(2).ok_or("missing checkpoint")?)?;
            let output = a.get(3).ok_or("missing output")?;
            let games = val(&a, 4, 500usize);
            let seed = val(&a, 5, 42u64);
            let min_ply = val(&a, 6, 18u8);
            let mut hard = BTreeSet::new();
            let mut rng = Rng::new(seed);
            for g in 0..games {
                let model_first = g % 2 == 0;
                let mut board = neurofour::connect4::Board::default();
                let mut sequence = String::new();
                let mut candidates = Vec::new();
                let mut model = SparseAgent { net: &net };
                let mut search = SearchAgent { depth: 4 };
                while !board.terminal() {
                    let model_turn = (board.turn == 1) == model_first;
                    let actual = if model_turn {
                        let mc = model.choose(&board, &mut rng);
                        let sc = search.choose(&board, &mut rng);
                        if board.moves >= min_ply && mc != sc {
                            candidates.push(sequence.clone());
                        }
                        mc
                    } else {
                        search.choose(&board, &mut rng)
                    };
                    board.play(actual)?;
                    sequence.push(char::from_digit((actual + 1) as u32, 10).unwrap());
                }
                let model_lost = match board.winner() {
                    1 => !model_first,
                    -1 => model_first,
                    _ => false,
                };
                if model_lost {
                    hard.extend(candidates);
                }
            }
            let mut f = File::create(output).map_err(|e| e.to_string())?;
            for s in &hard {
                writeln!(f, "{s}").map_err(|e| e.to_string())?;
            }
            println!(
                "mined games={games} unique_hard_positions={} file={output}",
                hard.len()
            );
        }
        Some("train") => {
            let data = a.get(2).ok_or("missing dataset")?;
            let out = a.get(3).ok_or("missing checkpoint")?;
            let epochs = val(&a, 4, 3);
            let seed = val(&a, 5, 42);
            let mut cfg = Config::default();
            cfg.max_nodes = val(&a, 6, cfg.max_nodes);
            cfg.top_k = val(&a, 7, cfg.top_k);
            cfg.max_edges_per_node = val(&a, 8, cfg.max_edges_per_node);
            let patience: usize = val(&a, 9, 2);
            let samples = dataset::load(data)?;
            let (train, validation) = dataset::split_by_game(&samples, 10);
            if train.is_empty() || validation.is_empty() {
                return Err("train/validation split is empty".into());
            }
            let train = dataset::augment_mirrors(&train);
            let mut net = SparseNet::new(cfg, seed);
            let t = Instant::now();
            let mut rng = Rng::new(seed);
            let mut best = net.clone();
            let mut best_val = f32::INFINITY;
            let mut stale = 0usize;
            let mut completed = 0usize;
            for epoch in 1..=epochs {
                let train_loss = net.train_epoch(&train, &mut rng);
                let val_loss = net.mse(&validation);
                let mut probe = SparseAgent { net: &net };
                let mut heuristic = HeuristicAgent;
                let arena_result = arena::play(&mut probe, &mut heuristic, 20, seed + 10_000);
                println!("epoch={epoch} train_loss={train_loss:.6} val_loss={val_loss:.6} validation_heuristic_score={:.3} nodes={} edges={}",arena_result.score(),net.nodes.len(),net.edge_count());
                completed = epoch;
                if val_loss + 1e-5 < best_val {
                    best_val = val_loss;
                    best = net.clone();
                    stale = 0;
                } else {
                    stale += 1;
                    if stale >= patience {
                        println!("early_stop epoch={epoch} patience={patience}");
                        break;
                    }
                }
            }
            net = best;
            net.save(out)?;
            println!("trained samples={} augmented_train={} validation={} epochs_completed={} best_val_loss={:.6} nodes={} edges={} time_ms={} peak_ram_kb={} checkpoint={}",samples.len(),train.len(),validation.len(),completed,best_val,net.nodes.len(),net.edge_count(),t.elapsed().as_millis(),memory_kb(),out);
        }
        Some("train-policy") => {
            let input = a.get(2).ok_or("missing Zenodo CSV")?;
            let output = a.get(3).ok_or("missing policy output")?;
            let limit = val(&a, 4, 100_000usize);
            let epochs = val(&a, 5, 3usize);
            let seed = val(&a, 6, 42u64);
            let mut samples = dataset::load_zenodo_policy(input, limit)?;
            if let Some(hard_path) = a.get(7) {
                let hard = dataset::load_solver_policy(hard_path)?;
                let repeat = val(&a, 8, 1usize);
                for _ in 0..repeat {
                    samples.extend(hard.iter().cloned());
                }
            }
            let mut policy = PolicyNet::new();
            let t = Instant::now();
            let loss = policy.train(&samples, epochs, seed, 0.01);
            policy.save(output)?;
            println!(
                "trained_policy samples={} epochs={} loss={loss:.6} time_ms={} file={output}",
                samples.len(),
                epochs,
                t.elapsed().as_millis()
            );
        }
        Some("go-generate") => {
            let size = val(&a, 2, 9usize);
            let games = val(&a, 3, 100usize);
            let file = a.get(4).ok_or("missing Go data file")?;
            let seed = val(&a, 5, 42u64);
            let t = Instant::now();
            neurofour::go::generate(file, size, games, seed)?;
            println!(
                "generated_go size={size} games={games} time_ms={} file={file}",
                t.elapsed().as_millis()
            );
        }
        Some("go-train") => {
            let data = a.get(2).ok_or("missing Go data")?;
            let output = a.get(3).ok_or("missing Go model")?;
            let epochs = val(&a, 4, 2usize);
            let seed = val(&a, 5, 42u64);
            let games = neurofour::go::load_games(data)?;
            let size = games.first().ok_or("empty Go data")?.size;
            let mut model = neurofour::go::GoModel::new(size);
            let t = Instant::now();
            let loss = model.train(&games, epochs, seed)?;
            model.save(output)?;
            println!("trained_go size={size} games={} epochs={epochs} loss={loss:.6} time_ms={} peak_ram_kb={} file={output}",games.len(),t.elapsed().as_millis(),memory_kb());
        }
        Some("go-benchmark") => {
            let model = neurofour::go::GoModel::load(a.get(2).ok_or("missing Go model")?)?;
            let games = val(&a, 3, 20usize);
            let seed = val(&a, 4, 42u64);
            for heuristic in [false, true] {
                let t = Instant::now();
                let (w, l) = neurofour::go::benchmark(&model, games, seed, heuristic)?;
                println!("go_size={} opponent={} games={games} W-L={w}-{l} win_rate={:.3} time_ms={} peak_ram_kb={}",model.size,if heuristic{"heuristic"}else{"random"},w as f64/games as f64,t.elapsed().as_millis(),memory_kb());
            }
        }
        Some("eval") => {
            let cp = a.get(2).ok_or("missing checkpoint")?;
            let net = SparseNet::load(cp)?;
            let name = a.get(3).map(String::as_str).unwrap_or("heuristic");
            let games = val(&a, 4, 100);
            let seed = val(&a, 5, 42);
            let depth = val(&a, 6, 4);
            let mut model = SparseAgent { net: &net };
            let mut opp = opponent(name, depth);
            let r = arena::play(&mut model, opp.as_mut(), games, seed);
            println!("agent=sparse opponent={} games={} wins={} losses={} draws={} score={:.3} elo={:+.1} time_ms={} games_per_sec={:.1} nodes={} edges={} peak_ram_kb={}",opp.name(),games,r.a_wins,r.b_wins,r.draws,r.score(),r.elo(),r.elapsed.as_millis(),games as f64/r.elapsed.as_secs_f64().max(1e-9),net.nodes.len(),net.edge_count(),memory_kb());
        }
        Some("benchmark") => {
            let cp = a.get(2).ok_or("missing checkpoint")?;
            let net = SparseNet::load(cp)?;
            let games = val(&a, 3, 100);
            let seed = val(&a, 4, 42);
            for (i, name) in ["random", "heuristic", "search"].iter().enumerate() {
                let mut model = SparseAgent { net: &net };
                let mut opp = opponent(name, 4);
                let r = arena::play(&mut model, opp.as_mut(), games, seed + i as u64);
                println!("opponent={name} games={games} W-L-D={}-{}-{} score={:.3} elo={:+.1} time_ms={}",r.a_wins,r.b_wins,r.draws,r.score(),r.elo(),r.elapsed.as_millis());
            }
            println!(
                "nodes={} edges={} peak_ram_kb={}",
                net.nodes.len(),
                net.edge_count(),
                memory_kb()
            );
        }
        Some("benchmark-search") => {
            let cp = a.get(2).ok_or("missing checkpoint")?;
            let net = SparseNet::load(cp)?;
            let depth = val(&a, 3, 2u8);
            let games = val(&a, 4, 100usize);
            let seed = val(&a, 5, 42u64);
            for (i, name) in ["random", "heuristic", "search"].iter().enumerate() {
                let mut model = SparseSearchAgent { net: &net, depth };
                let mut opp = opponent(name, 4);
                let r = arena::play(&mut model, opp.as_mut(), games, seed + i as u64);
                println!("agent=sparse-search depth={depth} opponent={name} games={games} W-L-D={}-{}-{} score={:.3} elo={:+.1} time_ms={}",r.a_wins,r.b_wins,r.draws,r.score(),r.elo(),r.elapsed.as_millis());
            }
        }
        Some("benchmark-hybrid") => {
            let value = SparseNet::load(a.get(2).ok_or("missing value checkpoint")?)?;
            let policy = PolicyNet::load(a.get(3).ok_or("missing policy checkpoint")?)?;
            let depth = val(&a, 4, 2u8);
            let policy_weight = val(&a, 5, 0.1f32);
            let games = val(&a, 6, 100usize);
            let seed = val(&a, 7, 42u64);
            for (i, name) in ["random", "heuristic", "search"].iter().enumerate() {
                let mut model = HybridAgent {
                    value: &value,
                    policy: &policy,
                    depth,
                    policy_weight,
                };
                let mut opp = opponent(name, 4);
                let r = arena::play(&mut model, opp.as_mut(), games, seed + i as u64);
                println!("agent=hybrid depth={depth} policy_weight={policy_weight} opponent={name} games={games} W-L-D={}-{}-{} score={:.3} time_ms={}",r.a_wins,r.b_wins,r.draws,r.score(),r.elapsed.as_millis());
            }
        }
        Some("inspect") => {
            let cp = a.get(2).ok_or("missing checkpoint")?;
            let n = SparseNet::load(cp)?;
            println!(
                "step={} nodes={} active={} edges={} top_k={} max_nodes={} max_edges_per_node={}",
                n.step,
                n.nodes.len(),
                n.nodes.iter().filter(|x| !x.dormant).count(),
                n.edge_count(),
                n.cfg.top_k,
                n.cfg.max_nodes,
                n.cfg.max_edges_per_node
            );
        }
        _ => {
            usage();
            return Err("invalid command".into());
        }
    }
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        process::exit(1)
    }
}
