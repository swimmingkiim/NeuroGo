use crate::{
    baseline::{Agent, HeuristicAgent, RandomAgent, SearchAgent},
    connect4::Board,
    rng::Rng,
};
use std::{
    fs::File,
    io::{BufRead, BufReader, BufWriter, Write},
};

#[derive(Clone)]
pub struct Sample {
    pub board: Board,
    pub value: f32,
    pub game_id: u64,
}
pub fn generate(path: &str, games: usize, seed: u64) -> Result<usize, String> {
    let file = File::create(path).map_err(|e| e.to_string())?;
    let mut out = BufWriter::new(file);
    let mut rng = Rng::new(seed);
    let mut count = 0;
    for g in 0..games {
        let mut b = Board::default();
        let mut hist = Vec::new();
        let mut first: Box<dyn Agent> = match g % 6 {
            0 | 3 => Box::new(RandomAgent),
            1 | 4 => Box::new(HeuristicAgent),
            _ => Box::new(SearchAgent { depth: 3 }),
        };
        let mut second: Box<dyn Agent> = match (g / 2) % 3 {
            0 => Box::new(RandomAgent),
            1 => Box::new(HeuristicAgent),
            _ => Box::new(SearchAgent { depth: 3 }),
        };
        while !b.terminal() {
            hist.push(b);
            let c = if rng.usize(10) == 0 {
                let legal = b.legal_moves();
                legal[rng.usize(legal.len())]
            } else if b.turn == 1 {
                first.choose(&b, &mut rng)
            } else {
                second.choose(&b, &mut rng)
            };
            b.play(c)?;
        }
        let w = b.winner();
        for h in hist {
            let v = if w == 0 {
                0
            } else if w == h.turn {
                1
            } else {
                -1
            };
            let seq: String = h
                .cells
                .iter()
                .map(|x| match x {
                    1 => '1',
                    -1 => '2',
                    _ => '0',
                })
                .collect();
            writeln!(out, "{},{},{},{}", seq, h.turn, v, g).map_err(|e| e.to_string())?;
            count += 1;
        }
    }
    Ok(count)
}
pub fn load(path: &str) -> Result<Vec<Sample>, String> {
    let f = File::open(path).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for line in BufReader::new(f).lines() {
        let line = line.map_err(|e| e.to_string())?;
        let p: Vec<_> = line.trim().split(',').collect();
        if !(p.len() == 3 || p.len() == 4) || p[0].len() != 42 {
            return Err("invalid dataset row".into());
        }
        let mut b = Board::default();
        for (i, ch) in p[0].chars().enumerate() {
            b.cells[i] = match ch {
                '0' => 0,
                '1' => 1,
                '2' => -1,
                _ => return Err("invalid cell".into()),
            };
        }
        b.turn = p[1].parse().map_err(|_| "invalid turn")?;
        b.moves = b.cells.iter().filter(|&&x| x != 0).count() as u8;
        out.push(Sample {
            board: b,
            value: p[2].parse().map_err(|_| "invalid value")?,
            game_id: if p.len() == 4 {
                p[3].parse().map_err(|_| "invalid game id")?
            } else {
                out.len() as u64
            },
        });
    }
    Ok(out)
}

pub fn save(path: &str, samples: &[Sample]) -> Result<(), String> {
    let mut out = BufWriter::new(File::create(path).map_err(|e| e.to_string())?);
    for sample in samples {
        let encoded: String = sample
            .board
            .cells
            .iter()
            .map(|x| match x {
                1 => '1',
                -1 => '2',
                _ => '0',
            })
            .collect();
        writeln!(
            out,
            "{encoded},{},{},{}",
            sample.board.turn, sample.value, sample.game_id
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn import_uci(input: &str, output: &str) -> Result<usize, String> {
    let src = BufReader::new(File::open(input).map_err(|e| e.to_string())?);
    let mut out = BufWriter::new(File::create(output).map_err(|e| e.to_string())?);
    let mut count = 0usize;
    for line in src.lines() {
        let line = line.map_err(|e| e.to_string())?;
        let fields: Vec<_> = line.trim().split(',').collect();
        if fields.len() != 43 {
            return Err(format!("invalid UCI row {}", count + 1));
        }
        let mut board = ['0'; 42];
        // UCI is column-major a1..a6,b1..; NeuroFour is row-major bottom-up.
        for col in 0..7 {
            for row in 0..6 {
                board[row * 7 + col] = match fields[col * 6 + row] {
                    "x" => '1',
                    "o" => '2',
                    "b" => '0',
                    _ => return Err("invalid UCI cell".into()),
                };
            }
        }
        let value = match fields[42] {
            "win" => 1,
            "loss" => -1,
            "draw" => 0,
            _ => return Err("invalid UCI class".into()),
        };
        let encoded: String = board.iter().collect();
        let mut mirrored = ['0'; 42];
        for row in 0..6 {
            for col in 0..7 {
                mirrored[row * 7 + col] = board[row * 7 + (6 - col)];
            }
        }
        let mirrored: String = mirrored.iter().collect();
        let canonical = if encoded <= mirrored {
            &encoded
        } else {
            &mirrored
        };
        let mut group = 0xcbf29ce484222325u64;
        for byte in canonical.bytes() {
            group ^= byte as u64;
            group = group.wrapping_mul(0x100000001b3);
        }
        writeln!(out, "{encoded},1,{value},{group}").map_err(|e| e.to_string())?;
        count += 1;
    }
    Ok(count)
}

#[derive(Debug)]
pub struct PolicyImportStats {
    pub positions: usize,
    pub samples: usize,
    pub skipped: usize,
}

#[derive(Clone)]
pub struct PolicySample {
    pub board: Board,
    pub targets: [f32; 7],
}

pub fn load_zenodo_policy(input: &str, limit: usize) -> Result<Vec<PolicySample>, String> {
    let src = BufReader::new(File::open(input).map_err(|e| e.to_string())?);
    let mut samples = Vec::with_capacity(limit);
    for (line_no, line) in src.lines().enumerate() {
        let line = line.map_err(|e| e.to_string())?;
        if line_no == 0 {
            continue;
        }
        if samples.len() >= limit {
            break;
        }
        let p: Vec<_> = line.trim().split(',').collect();
        if p.len() != 10 || p[4].len() != 42 {
            continue;
        }
        let mut board = Board::default();
        for source_row in 0..6 {
            for col in 0..7 {
                let internal_row = 5 - source_row;
                board.cells[internal_row * 7 + col] = match p[4].as_bytes()[source_row * 7 + col] {
                    b'0' => 0,
                    b'1' => 1,
                    b'2' => -1,
                    _ => return Err("invalid Zenodo board".into()),
                };
            }
        }
        board.turn = if p[1] == "1" { 1 } else { -1 };
        board.moves = board.cells.iter().filter(|&&x| x != 0).count() as u8;
        if board.terminal() {
            continue;
        }
        let legal = board.legal_moves();
        let best = [5usize, 9, 8, 7, 6]
            .iter()
            .filter_map(|&i| p[i].parse::<i32>().ok())
            .find(|&m| m >= 0 && legal.contains(&(m as usize)));
        if let Some(best_move) = best {
            let mut targets = [0.0; 7];
            targets[best_move as usize] = 1.0;
            samples.push(PolicySample { board, targets });
        }
    }
    Ok(samples)
}

pub fn load_solver_policy(input: &str) -> Result<Vec<PolicySample>, String> {
    let src = BufReader::new(File::open(input).map_err(|e| e.to_string())?);
    let mut samples = Vec::new();
    for line in src.lines() {
        let line = line.map_err(|e| e.to_string())?;
        let p: Vec<_> = line.split_whitespace().collect();
        if p.len() != 8 {
            continue;
        }
        let mut board = Board::default();
        for ch in p[0].chars() {
            let col = ch.to_digit(10).ok_or("invalid solver move")? as usize;
            board.play(col.checked_sub(1).ok_or("invalid solver move")?)?;
        }
        if board.terminal() {
            continue;
        }
        let scores: Vec<i32> = p[1..]
            .iter()
            .map(|s| s.parse().map_err(|_| "invalid solver analysis"))
            .collect::<Result<_, _>>()?;
        let legal = board.legal_moves();
        let best = legal
            .iter()
            .map(|&c| scores[c])
            .max()
            .ok_or("no legal move")?;
        let winners: Vec<_> = legal.into_iter().filter(|&c| scores[c] == best).collect();
        let mut targets = [0.0; 7];
        for c in winners.iter().copied() {
            targets[c] = 1.0 / winners.len() as f32;
        }
        samples.push(PolicySample { board, targets });
    }
    Ok(samples)
}

pub fn import_zenodo_policy(
    input: &str,
    output: &str,
    limit: usize,
) -> Result<PolicyImportStats, String> {
    let src = BufReader::new(File::open(input).map_err(|e| e.to_string())?);
    let mut out = BufWriter::new(File::create(output).map_err(|e| e.to_string())?);
    let mut stats = PolicyImportStats {
        positions: 0,
        samples: 0,
        skipped: 0,
    };
    for (line_no, line) in src.lines().enumerate() {
        let line = line.map_err(|e| e.to_string())?;
        if line_no == 0 {
            continue;
        }
        if stats.positions >= limit {
            break;
        }
        let p: Vec<_> = line.trim().split(',').collect();
        if p.len() != 10 || p[4].len() != 42 {
            stats.skipped += 1;
            continue;
        }
        let mut board = Board::default();
        for source_row in 0..6 {
            for col in 0..7 {
                let ch = p[4].as_bytes()[source_row * 7 + col];
                let internal_row = 5 - source_row;
                board.cells[internal_row * 7 + col] = match ch {
                    b'0' => 0,
                    b'1' => 1,
                    b'2' => -1,
                    _ => return Err(format!("invalid board at line {}", line_no + 1)),
                };
            }
        }
        board.turn = match p[1] {
            "1" => 1,
            "2" => -1,
            _ => {
                stats.skipped += 1;
                continue;
            }
        };
        board.moves = board.cells.iter().filter(|&&x| x != 0).count() as u8;
        if board.terminal() {
            stats.skipped += 1;
            continue;
        }
        let legal = board.legal_moves();
        let best = [5usize, 9, 8, 7, 6]
            .iter()
            .filter_map(|&i| p[i].parse::<i32>().ok())
            .find(|&m| m >= 0 && legal.contains(&(m as usize)))
            .map(|m| m as usize);
        let Some(best) = best else {
            stats.skipped += 1;
            continue;
        };
        let mut group = 0xcbf29ce484222325u64;
        for byte in p[0].bytes() {
            group ^= byte as u64;
            group = group.wrapping_mul(0x100000001b3);
        }
        let alternatives: Vec<_> = legal.iter().copied().filter(|&m| m != best).collect();
        if alternatives.is_empty() {
            stats.skipped += 1;
            continue;
        }
        let alternative = alternatives[group as usize % alternatives.len()];
        for (mv, value) in [(best, -1), (alternative, 1)] {
            let mut after = board;
            after.play(mv)?;
            let encoded: String = after
                .cells
                .iter()
                .map(|x| match x {
                    1 => '1',
                    -1 => '2',
                    _ => '0',
                })
                .collect();
            writeln!(out, "{encoded},{},{value},{group}", after.turn).map_err(|e| e.to_string())?;
            stats.samples += 1;
        }
        stats.positions += 1;
    }
    Ok(stats)
}

pub fn import_pons_value(input: &str, output: &str) -> Result<usize, String> {
    let text = std::fs::read_to_string(input).map_err(|e| e.to_string())?;
    let mut out = BufWriter::new(File::create(output).map_err(|e| e.to_string())?);
    let mut cursor = 0usize;
    let mut count = 0usize;
    while let Some(relative) = text[cursor..].find("\"moves\": \"") {
        let start = cursor + relative + "\"moves\": \"".len();
        let end = start + text[start..].find('"').ok_or("invalid Pons moves")?;
        let moves = &text[start..end];
        let scores_marker = text[end..]
            .find("\"scores\": [")
            .ok_or("missing Pons scores")?;
        let scores_start = end + scores_marker + "\"scores\": [".len();
        let scores_end = scores_start
            + text[scores_start..]
                .find(']')
                .ok_or("invalid Pons scores")?;
        let scores: Vec<i32> = text[scores_start..scores_end]
            .split(',')
            .map(|s| {
                s.trim()
                    .parse()
                    .map_err(|_| "invalid Pons score".to_string())
            })
            .collect::<Result<_, _>>()?;
        if scores.len() != 7 {
            return Err("Pons scores must contain seven columns".into());
        }
        let mut board = Board::default();
        for ch in moves.chars() {
            let col = ch.to_digit(10).ok_or("invalid Pons move")? as usize;
            board.play(col)?;
            if board.terminal() {
                return Err("Pons sequence reaches terminal state".into());
            }
        }
        let best = board
            .legal_moves()
            .into_iter()
            .map(|col| scores[col])
            .filter(|&score| score != -1000)
            .max()
            .ok_or("Pons position has no legal score")?;
        let value = best.signum();
        let encoded: String = board
            .cells
            .iter()
            .map(|x| match x {
                1 => '1',
                -1 => '2',
                _ => '0',
            })
            .collect();
        let mirrored: String = board
            .mirror()
            .cells
            .iter()
            .map(|x| match x {
                1 => '1',
                -1 => '2',
                _ => '0',
            })
            .collect();
        let canonical = if encoded <= mirrored {
            &encoded
        } else {
            &mirrored
        };
        let mut group = 0xcbf29ce484222325u64;
        for byte in canonical.bytes() {
            group ^= byte as u64;
            group = group.wrapping_mul(0x100000001b3);
        }
        writeln!(out, "{encoded},{},{value},{group}", board.turn).map_err(|e| e.to_string())?;
        count += 1;
        cursor = scores_end + 1;
    }
    Ok(count)
}

pub fn import_solver_values(input: &str, output: &str) -> Result<usize, String> {
    let src = BufReader::new(File::open(input).map_err(|e| e.to_string())?);
    let mut samples = Vec::new();
    for line in src.lines() {
        let line = line.map_err(|e| e.to_string())?;
        let p: Vec<_> = line.split_whitespace().collect();
        if p.len() < 2 {
            continue;
        }
        let mut board = Board::default();
        for ch in p[0].chars() {
            let one_based = ch.to_digit(10).ok_or("invalid solver sequence")? as usize;
            if !(1..=7).contains(&one_based) {
                return Err("solver move out of range".into());
            }
            board.play(one_based - 1)?;
        }
        if board.terminal() {
            continue;
        }
        let score: i32 = p[1].parse().map_err(|_| "invalid solver score")?;
        let encoded: String = board
            .cells
            .iter()
            .map(|x| match x {
                1 => '1',
                -1 => '2',
                _ => '0',
            })
            .collect();
        let mirrored: String = board
            .mirror()
            .cells
            .iter()
            .map(|x| match x {
                1 => '1',
                -1 => '2',
                _ => '0',
            })
            .collect();
        let canonical = if encoded <= mirrored {
            &encoded
        } else {
            &mirrored
        };
        let mut group = 0xcbf29ce484222325u64;
        for byte in canonical.bytes() {
            group ^= byte as u64;
            group = group.wrapping_mul(0x100000001b3);
        }
        samples.push(Sample {
            board,
            value: score.signum() as f32,
            game_id: group,
        });
    }
    save(output, &samples)?;
    Ok(samples.len())
}

pub fn split_by_game(data: &[Sample], validation_percent: u64) -> (Vec<Sample>, Vec<Sample>) {
    let mut train = Vec::new();
    let mut validation = Vec::new();
    for sample in data {
        // Multiplicative hashing avoids depending on input order while keeping
        // every state from one game in exactly one partition.
        let bucket = sample.game_id.wrapping_mul(0x9E3779B97F4A7C15) % 100;
        if bucket < validation_percent {
            validation.push(sample.clone());
        } else {
            train.push(sample.clone());
        }
    }
    (train, validation)
}

pub fn augment_mirrors(data: &[Sample]) -> Vec<Sample> {
    let mut out = Vec::with_capacity(data.len() * 2);
    for sample in data {
        out.push(sample.clone());
        out.push(Sample {
            board: sample.board.mirror(),
            value: sample.value,
            game_id: sample.game_id,
        });
    }
    out
}
