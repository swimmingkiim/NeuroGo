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
    eprintln!(concat!(
        "neurofour <command>\n",
        "  generate <file> [games] [seed]\n",
        "  import-uci <uci.csv> <native.csv>\n",
        "  import-pons <pons.json> <native.csv>\n",
        "  import-solver <solver-output> <native.csv>\n",
        "  import-zenodo <optimal.csv> <native.csv> [position_limit=100000]\n",
        "  merge <output.csv> <input.csv>...\n",
        "  mine-hard <checkpoint> <sequences.txt> [games=500] [seed=42] [min_ply=18]\n",
        "  train <data> <checkpoint> [max_epochs] [seed] [max_nodes] [top_k] [max_edges_per_node] [patience]\n",
        "  train-policy <zenodo.csv> <policy> [positions=100000] [epochs=3] [seed=42] [hard_analysis] [hard_repeat=1]\n",
        "  go-generate <size> <games> <file> [seed=42]\n",
        "  go-import-sgf <sgf-file-or-directory> <data.csv> [skip_invalid=false]\n",
        "  go-export-sgf <data.csv> <sgf-file>\n",
        "  go-train <data> <model.gns> [epochs=2] [seed=42] [validation_fraction=0.1] [split_seed=42] [max_nodes=128] [top_k=8] [max_edges=16] [error_threshold=0.55] [novelty=0.2] [prune_strength=0.01] [growth_rule=0|1|2] [spatial_radius=0] [activation_balance=0.2] [value_learning_rate=0.00001] [score_learning_rate=0.0001] [global_node_fraction=0.25] [mcts_candidates=24]\n",
        "  go-distill <teacher-train.tsv> <teacher-validation.tsv> <model.gns> [epochs=5] [seed=42] [max_nodes=128] [top_k=8] [max_edges=16] [spatial_radius=0] [activation_balance=0.2] [global_node_fraction=0.25] [mcts_candidates=24] [score_learning_rate=0.0001]\n",
        "  go-evaluate <model> <data.csv>\n",
        "  go-benchmark <model> [games=20] [seed=42] [komi=6.5]\n",
        "  go-benchmark-mcts <model> <games> <seed> <komi> <simulations> [cpuct=1.4] [opponent=heuristic|random]\n",
        "  go-benchmark-gtp-mcts <model> <engine> <games> <seed> <komi> <simulations> <cpuct> [mcts_candidates] [engine-arg ...]\n",
        "  go-benchmark-gtp <model> <engine> <games> <seed> <komi> [engine-arg ...]\n",
        "  eval <checkpoint> [opponent=random|heuristic|search] [games] [seed] [depth]\n",
        "  benchmark <checkpoint> [games] [seed]\n",
        "  benchmark-search <checkpoint> [model_depth=2] [games=100] [seed=42]\n",
        "  benchmark-hybrid <value> <policy> [depth=2] [policy_weight=.1] [games=100] [seed=42]\n",
        "  inspect <checkpoint>"
    ));
}
fn val<T: std::str::FromStr>(xs: &[String], i: usize, default: T) -> T {
    xs.get(i).and_then(|s| s.parse().ok()).unwrap_or(default)
}
fn parse_optional<T: std::str::FromStr>(
    xs: &[String],
    i: usize,
    name: &str,
) -> Result<Option<T>, String> {
    xs.get(i)
        .map(|value| {
            value
                .parse()
                .map_err(|_| format!("invalid {name}: {value}"))
        })
        .transpose()
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
        Some("go-import-sgf") => {
            let input = a.get(2).ok_or("missing SGF file or directory")?;
            let output = a.get(3).ok_or("missing Go data output")?;
            let skip_invalid = match a.get(4).map(String::as_str) {
                None | Some("false") => false,
                Some("true") => true,
                Some(_) => return Err("skip_invalid must be true or false".into()),
            };
            let stats = neurofour::go::import_sgf_with_options(input, output, skip_invalid)?;
            for example in &stats.invalid_examples {
                eprintln!("skipped_invalid_sgf {example}");
            }
            println!(
                "imported_sgf games={} skipped={} invalid={} file={output}",
                stats.imported, stats.skipped, stats.invalid
            );
        }
        Some("go-export-sgf") => {
            let input = a.get(2).ok_or("missing Go data file")?;
            let output = a.get(3).ok_or("missing SGF output file")?;
            let games = neurofour::go::export_sgf(input, output)?;
            println!("exported_sgf games={games} file={output}");
        }
        Some("go-train") => {
            let data = a.get(2).ok_or("missing Go data")?;
            let output = a.get(3).ok_or("missing Go model")?;
            let epochs = val(&a, 4, 2usize);
            let seed = val(&a, 5, 42u64);
            let validation_fraction = a
                .get(6)
                .map(|value| {
                    value
                        .parse::<f32>()
                        .map_err(|_| "invalid Go validation fraction")
                })
                .transpose()?
                .unwrap_or(0.1);
            let split_seed = val(&a, 7, 42u64);
            let mut config = neurofour::go::GoSparseConfig::default();
            config.max_nodes = parse_optional(&a, 8, "max_nodes")?.unwrap_or(config.max_nodes);
            config.top_k = parse_optional(&a, 9, "top_k")?.unwrap_or(config.top_k);
            config.max_edges_per_node =
                parse_optional(&a, 10, "max_edges_per_node")?.unwrap_or(config.max_edges_per_node);
            config.error_threshold =
                parse_optional(&a, 11, "error_threshold")?.unwrap_or(config.error_threshold);
            config.novelty_threshold =
                parse_optional(&a, 12, "novelty_threshold")?.unwrap_or(config.novelty_threshold);
            config.prune_strength =
                parse_optional(&a, 13, "prune_strength")?.unwrap_or(config.prune_strength);
            config.growth_rule =
                parse_optional(&a, 14, "growth_rule")?.unwrap_or(config.growth_rule);
            config.spatial_radius =
                parse_optional(&a, 15, "spatial_radius")?.unwrap_or(config.spatial_radius);
            config.activation_balance =
                parse_optional(&a, 16, "activation_balance")?.unwrap_or(config.activation_balance);
            config.value_learning_rate = parse_optional(&a, 17, "value_learning_rate")?
                .unwrap_or(config.value_learning_rate);
            config.score_learning_rate = parse_optional(&a, 18, "score_learning_rate")?
                .unwrap_or(config.score_learning_rate);
            config.global_node_fraction = parse_optional(&a, 19, "global_node_fraction")?
                .unwrap_or(config.global_node_fraction);
            config.mcts_candidates =
                parse_optional(&a, 20, "mcts_candidates")?.unwrap_or(config.mcts_candidates);
            let games = neurofour::go::load_games(data)?;
            let size = games.first().ok_or("empty Go data")?.size;
            let (training_games, validation_games) =
                neurofour::go::split_games(&games, size, validation_fraction, split_seed)?;
            let mut model = neurofour::go::GoSparseModel::new(size, config, seed)?;
            let t = Instant::now();
            let validation_baseline = if validation_games.is_empty() {
                None
            } else {
                Some(model.evaluate_loss(&validation_games)?.0)
            };
            let (loss, best_epoch) = if validation_games.is_empty() {
                model.train(&training_games, epochs, seed)?;
                (model.evaluate_loss(&training_games)?.0, None)
            } else {
                let (best_epoch, loss, _) = model.train_with_validation(
                    &training_games,
                    &validation_games,
                    epochs,
                    seed,
                )?;
                (loss, Some(best_epoch))
            };
            let (validation_loss, validation_moves) = if validation_games.is_empty() {
                (None, 0)
            } else {
                let (loss, moves) = model.evaluate_loss(&validation_games)?;
                (Some(loss), moves)
            };
            model.save(output)?;
            let validation_loss = validation_loss
                .map(|loss| format!("{loss:.6}"))
                .unwrap_or_else(|| "disabled".into());
            let validation_baseline = validation_baseline
                .map(|loss| format!("{loss:.6}"))
                .unwrap_or_else(|| "disabled".into());
            let best_epoch = best_epoch
                .map(|epoch| epoch.to_string())
                .unwrap_or_else(|| "disabled".into());
            let (usage_nodes, used_nodes, mean_usage, max_usage) = model.neuron_usage_stats();
            let (score_mse, score_moves) = model.evaluate_score_loss(&validation_games)?;
            let score_mse = score_mse
                .map(|mse| format!("{mse:.6}"))
                .unwrap_or_else(|| "unavailable".into());
            println!("trained_go architecture=dynamic_sparse size={size} nodes={} edges={} global_nodes={} top_k={} max_nodes={} max_edges_per_node={} error_threshold={} novelty_threshold={} prune_strength={} growth_rule={} spatial_radius={} global_node_fraction={} mcts_candidates={} activation_balance={} value_learning_rate={} score_learning_rate={} used_neurons={used_nodes}/{usage_nodes} mean_selection_rate={mean_usage:.6} max_selection_rate={max_usage:.6} training_symmetries=8 policy_objective=legal_softmax auxiliary_score_moves={score_moves} validation_score_mse={score_mse} early_stopping=policy_value_score_objective train_games={} validation_games={} epochs={epochs} best_epoch={best_epoch} train_loss={loss:.6} validation_baseline_loss={validation_baseline} validation_loss={validation_loss} validation_moves={validation_moves} time_ms={} peak_ram_kb={} file={output}",model.nodes.len(),model.edge_count(),model.nodes.iter().filter(|node| node.global).count(),model.cfg.top_k,model.cfg.max_nodes,model.cfg.max_edges_per_node,model.cfg.error_threshold,model.cfg.novelty_threshold,model.cfg.prune_strength,model.cfg.growth_rule,model.cfg.spatial_radius,model.cfg.global_node_fraction,model.cfg.mcts_candidates,model.cfg.activation_balance,model.cfg.value_learning_rate,model.cfg.score_learning_rate,training_games.len(),validation_games.len(),t.elapsed().as_millis(),memory_kb());
        }
        Some("go-distill") => {
            let training_path = a.get(2).ok_or("missing teacher training dataset")?;
            let validation_path = a.get(3).ok_or("missing teacher validation dataset")?;
            let output = a.get(4).ok_or("missing Go model output")?;
            let epochs = val(&a, 5, 5usize);
            let seed = val(&a, 6, 42u64);
            let training = neurofour::go::load_teacher_positions(training_path)?;
            let validation = neurofour::go::load_teacher_positions(validation_path)?;
            let size = training
                .first()
                .ok_or("empty teacher training dataset")?
                .size;
            if training.iter().any(|position| position.size != size)
                || validation.iter().any(|position| position.size != size)
            {
                return Err("teacher training and validation sets must use one board size".into());
            }
            let has_ownership_targets = |positions: &[neurofour::go::GoTeacherPosition]| {
                positions
                    .iter()
                    .flat_map(|position| &position.ownership)
                    .any(|&value| value != 0.0)
            };
            if !has_ownership_targets(&training) {
                return Err(
                    "teacher training ownership targets are all zero; verify the KataGo model supports ownership output"
                        .into(),
                );
            }
            if !has_ownership_targets(&validation) {
                return Err("teacher validation ownership targets are all zero".into());
            }
            let mut config = neurofour::go::GoSparseConfig::default();
            config.max_nodes = parse_optional(&a, 7, "max_nodes")?.unwrap_or(config.max_nodes);
            config.top_k = parse_optional(&a, 8, "top_k")?.unwrap_or(config.top_k);
            config.max_edges_per_node =
                parse_optional(&a, 9, "max_edges")?.unwrap_or(config.max_edges_per_node);
            config.spatial_radius =
                parse_optional(&a, 10, "spatial_radius")?.unwrap_or(config.spatial_radius);
            config.activation_balance =
                parse_optional(&a, 11, "activation_balance")?.unwrap_or(config.activation_balance);
            config.global_node_fraction = parse_optional(&a, 12, "global_node_fraction")?
                .unwrap_or(config.global_node_fraction);
            config.mcts_candidates =
                parse_optional(&a, 13, "mcts_candidates")?.unwrap_or(config.mcts_candidates);
            config.score_learning_rate = parse_optional(&a, 14, "score_learning_rate")?
                .unwrap_or(config.score_learning_rate);
            let mut model = neurofour::go::GoSparseModel::new(size, config, seed)?;
            let baseline = model.evaluate_teacher_metrics(&validation)?;
            let t = Instant::now();
            let (training_policy_loss, training_ownership_mse) =
                model.train_teacher(&training, epochs, seed)?;
            let (validation_policy_loss, validation_ownership_mse) =
                model.evaluate_teacher_metrics(&validation)?;
            model.save(output)?;
            println!(
                "go_distill board_size={size} train_positions={} validation_positions={} epochs={epochs} seed={seed} nodes={} edges={} train_policy_ce={training_policy_loss:.6} train_ownership_mse={training_ownership_mse:.6} validation_policy_ce_before={:.6} validation_policy_ce={validation_policy_loss:.6} validation_ownership_mse_before={:.6} validation_ownership_mse={validation_ownership_mse:.6} elapsed_ms={} model={output}",
                training.len(),
                validation.len(),
                model.nodes.len(),
                model.edge_count(),
                baseline.0,
                baseline.1,
                t.elapsed().as_millis(),
            );
        }
        Some("go-evaluate") => {
            let model = neurofour::go::GoSparseModel::load(a.get(2).ok_or("missing Go model")?)?;
            let games = neurofour::go::load_games(a.get(3).ok_or("missing Go data")?)?;
            let matching_games = games.iter().filter(|game| game.size == model.size).count();
            let (value_mse, value_moves) = model.evaluate_loss(&games)?;
            let (score_mse, score_moves) = model.evaluate_score_loss(&games)?;
            let (policy_top1, random_top1, policy_moves) = model.evaluate_policy_top1(&games)?;
            let (usage_nodes, used_nodes, mean_usage, max_usage) = model.neuron_usage_stats();
            let score_mse = score_mse
                .map(|mse| format!("{mse:.6}"))
                .unwrap_or_else(|| "unavailable".into());
            println!("evaluated_go architecture=dynamic_sparse size={} games={matching_games} nodes={} edges={} global_nodes={} used_neurons={used_nodes}/{usage_nodes} mean_selection_rate={mean_usage:.6} max_selection_rate={max_usage:.6} activation_balance={} score_learning_rate={} value_mse={value_mse:.6} value_moves={value_moves} score_mse={score_mse} score_moves={score_moves} policy_top1={policy_top1:.6} random_policy_top1={random_top1:.6} policy_moves={policy_moves}",model.size,model.nodes.len(),model.edge_count(),model.nodes.iter().filter(|node| node.global).count(),model.cfg.activation_balance,model.cfg.score_learning_rate);
        }
        Some("go-benchmark") => {
            let model = neurofour::go::GoSparseModel::load(a.get(2).ok_or("missing Go model")?)?;
            let games = val(&a, 3, 20usize);
            let seed = val(&a, 4, 42u64);
            let komi = val(&a, 5, 6.5f32);
            for heuristic in [false, true] {
                let t = Instant::now();
                let (w, l) = neurofour::go::benchmark_sparse_with_komi(
                    &model, games, seed, heuristic, komi,
                )?;
                println!("go_size={} architecture=dynamic_sparse opponent={} games={games} komi={komi} W-L={w}-{l} win_rate={:.3} time_ms={} peak_ram_kb={}",model.size,if heuristic{"heuristic"}else{"random"},w as f64/games as f64,t.elapsed().as_millis(),memory_kb());
            }
        }
        Some("go-benchmark-mcts") => {
            let model = neurofour::go::GoSparseModel::load(a.get(2).ok_or("missing Go model")?)?;
            let games = a
                .get(3)
                .ok_or("missing MCTS benchmark game count")?
                .parse::<usize>()
                .map_err(|_| "invalid MCTS benchmark game count")?;
            let seed = a
                .get(4)
                .ok_or("missing MCTS benchmark seed")?
                .parse::<u64>()
                .map_err(|_| "invalid MCTS benchmark seed")?;
            let komi = a
                .get(5)
                .ok_or("missing MCTS benchmark komi")?
                .parse::<f32>()
                .map_err(|_| "invalid MCTS benchmark komi")?;
            let simulations = a
                .get(6)
                .ok_or("missing MCTS simulation count")?
                .parse::<usize>()
                .map_err(|_| "invalid MCTS simulation count")?;
            let cpuct = parse_optional(&a, 7, "cpuct")?.unwrap_or(1.4f32);
            let opponent = a.get(8).map(String::as_str).unwrap_or("heuristic");
            let heuristic = match opponent {
                "heuristic" => true,
                "random" => false,
                _ => return Err("MCTS opponent must be heuristic or random".into()),
            };
            let t = Instant::now();
            let results = neurofour::go::benchmark_mcts_with_komi(
                &model,
                games,
                seed,
                simulations,
                cpuct,
                komi,
                heuristic,
            )?;
            let score = results.wins as f64 + results.draws as f64 * 0.5;
            println!("go_mcts_benchmark architecture=dynamic_sparse board_size={} opponent={opponent} simulations={simulations} cpuct={cpuct} games={games} komi={komi} W-L-D={}-{}-{} score={:.3} model_black={}-{}-{} model_white={}-{}-{} time_ms={} peak_ram_kb={}",model.size,results.wins,results.losses,results.draws,score/games as f64,results.model_black.0,results.model_black.1,results.model_black.2,results.model_white.0,results.model_white.1,results.model_white.2,t.elapsed().as_millis(),memory_kb());
        }
        Some("go-benchmark-gtp") => {
            let model = neurofour::go::GoSparseModel::load(a.get(2).ok_or("missing Go model")?)?;
            let engine = a.get(3).ok_or("missing GTP engine")?;
            let games = a
                .get(4)
                .ok_or("missing GTP benchmark game count")?
                .parse::<usize>()
                .map_err(|_| "invalid GTP benchmark game count")?;
            let seed = a
                .get(5)
                .ok_or("missing GTP benchmark seed")?
                .parse::<u64>()
                .map_err(|_| "invalid GTP benchmark seed")?;
            let komi = a
                .get(6)
                .ok_or("missing GTP benchmark komi")?
                .parse::<f32>()
                .map_err(|_| "invalid GTP benchmark komi")?;
            let t = Instant::now();
            let results = neurofour::go::benchmark_gtp(&model, engine, &a[7..], games, seed, komi)?;
            let (mean_margin, black_margin, white_margin) = results.score_margin_means();
            let margin_text = |margin: Option<f64>| {
                margin
                    .map(|margin| format!("{margin:.1}"))
                    .unwrap_or_else(|| "unavailable".into())
            };
            println!("go_gtp_benchmark architecture=dynamic_sparse board_size={} nodes={} edges={} engine={engine} games={games} W-L-D={}-{}-{} score={:.3} model_black={}-{}-{} model_white={}-{}-{} score_margin_mean={} score_margin_games={} black_score_margin_mean={} white_score_margin_mean={} final_score_disagreements={} time_ms={} peak_ram_kb={}",model.size,model.nodes.len(),model.edge_count(),results.wins,results.losses,results.draws,(results.wins as f64 + results.draws as f64 * 0.5) / games as f64,results.model_black.0,results.model_black.1,results.model_black.2,results.model_white.0,results.model_white.1,results.model_white.2,margin_text(mean_margin),results.score_margin_games,margin_text(black_margin),margin_text(white_margin),results.score_disagreements,t.elapsed().as_millis(),memory_kb());
        }
        Some("go-benchmark-gtp-mcts") => {
            let mut model =
                neurofour::go::GoSparseModel::load(a.get(2).ok_or("missing Go model")?)?;
            let engine = a.get(3).ok_or("missing GTP engine")?;
            let games = a
                .get(4)
                .ok_or("missing GTP benchmark game count")?
                .parse::<usize>()
                .map_err(|_| "invalid GTP benchmark game count")?;
            let seed = a
                .get(5)
                .ok_or("missing GTP benchmark seed")?
                .parse::<u64>()
                .map_err(|_| "invalid GTP benchmark seed")?;
            let komi = a
                .get(6)
                .ok_or("missing GTP benchmark komi")?
                .parse::<f32>()
                .map_err(|_| "invalid GTP benchmark komi")?;
            let simulations = a
                .get(7)
                .ok_or("missing MCTS simulation count")?
                .parse::<usize>()
                .map_err(|_| "invalid MCTS simulation count")?;
            let cpuct = a
                .get(8)
                .ok_or("missing MCTS cpuct")?
                .parse::<f32>()
                .map_err(|_| "invalid MCTS cpuct")?;
            let engine_arg_start =
                if let Some(value) = a.get(9).and_then(|value| value.parse::<usize>().ok()) {
                    if value == 0 || value > model.size * model.size + 1 {
                        return Err("invalid MCTS candidate count".into());
                    }
                    model.cfg.mcts_candidates = value;
                    10
                } else {
                    9
                };
            let t = Instant::now();
            let results = neurofour::go::benchmark_gtp_mcts(
                &model,
                engine,
                &a[engine_arg_start..],
                games,
                seed,
                komi,
                simulations,
                cpuct,
            )?;
            let (mean_margin, black_margin, white_margin) = results.score_margin_means();
            let margin_text = |margin: Option<f64>| {
                margin
                    .map(|margin| format!("{margin:.1}"))
                    .unwrap_or_else(|| "unavailable".into())
            };
            println!("go_gtp_mcts_benchmark architecture=dynamic_sparse board_size={} nodes={} edges={} engine={engine} simulations={simulations} mcts_candidates={} cpuct={cpuct} games={games} W-L-D={}-{}-{} score={:.3} model_black={}-{}-{} model_white={}-{}-{} score_margin_mean={} score_margin_games={} black_score_margin_mean={} white_score_margin_mean={} final_score_disagreements={} time_ms={} peak_ram_kb={}",model.size,model.nodes.len(),model.edge_count(),model.cfg.mcts_candidates,results.wins,results.losses,results.draws,(results.wins as f64 + results.draws as f64 * 0.5) / games as f64,results.model_black.0,results.model_black.1,results.model_black.2,results.model_white.0,results.model_white.1,results.model_white.2,margin_text(mean_margin),results.score_margin_games,margin_text(black_margin),margin_text(white_margin),results.score_disagreements,t.elapsed().as_millis(),memory_kb());
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
