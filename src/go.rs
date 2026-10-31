use crate::rng::Rng;
use std::{
    collections::{HashSet, VecDeque},
    fs::{self, File},
    io::{BufRead, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
    process::{ChildStdin, ChildStdout, Command, Stdio},
    sync::Arc,
};

#[derive(Clone, Debug)]
pub struct GoBoard {
    pub size: usize,
    pub cells: Vec<i8>,
    pub turn: i8,
    pub ko: Option<usize>,
    pub passes: u8,
    pub moves: usize,
    position_history: Arc<HashSet<u64>>,
}

impl GoBoard {
    pub fn new(size: usize) -> Result<Self, String> {
        if !(5..=19).contains(&size) {
            return Err("Go size must be 5..19".into());
        }
        let mut board = Self {
            size,
            cells: vec![0; size * size],
            turn: 1,
            ko: None,
            passes: 0,
            moves: 0,
            position_history: Arc::new(HashSet::new()),
        };
        let initial_hash = board.position_hash();
        Arc::make_mut(&mut board.position_history).insert(initial_hash);
        Ok(board)
    }
    pub fn with_setup(
        size: usize,
        black: &[usize],
        white: &[usize],
        turn: i8,
    ) -> Result<Self, String> {
        if !matches!(turn, -1 | 1) {
            return Err("Go turn must be -1 or 1".into());
        }
        let mut board = Self::new(size)?;
        for (color, points) in [(1, black), (-1, white)] {
            for &point in points {
                if point >= board.cells.len() || board.cells[point] != 0 {
                    return Err("invalid or overlapping Go setup stones".into());
                }
                board.cells[point] = color;
            }
        }
        board.turn = turn;
        let initial_hash = board.position_hash();
        Arc::make_mut(&mut board.position_history).clear();
        Arc::make_mut(&mut board.position_history).insert(initial_hash);
        Ok(board)
    }
    pub fn pass_action(&self) -> usize {
        self.cells.len()
    }
    fn neighbors(&self, p: usize) -> ([usize; 4], usize) {
        let r = p / self.size;
        let c = p % self.size;
        let mut neighbors = [0; 4];
        let mut count = 0;
        if r > 0 {
            neighbors[count] = p - self.size;
            count += 1;
        }
        if r + 1 < self.size {
            neighbors[count] = p + self.size;
            count += 1;
        }
        if c > 0 {
            neighbors[count] = p - 1;
            count += 1;
        }
        if c + 1 < self.size {
            neighbors[count] = p + 1;
            count += 1;
        }
        (neighbors, count)
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
            let (neighbors, count) = self.neighbors(p);
            for &n in &neighbors[..count] {
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
    fn group_liberties(&self) -> (Vec<usize>, Vec<usize>, Vec<u64>) {
        let mut group_at = vec![usize::MAX; self.cells.len()];
        let mut liberty_seen = vec![usize::MAX; self.cells.len()];
        let mut liberties = Vec::new();
        let mut group_hashes = Vec::new();
        let mut queue = VecDeque::new();

        for start in 0..self.cells.len() {
            if self.cells[start] == 0 || group_at[start] != usize::MAX {
                continue;
            }
            let group = liberties.len();
            let marker = group;
            let color = self.cells[start];
            let mut count = 0;
            let mut group_hash = 0;
            group_at[start] = group;
            queue.push_back(start);
            while let Some(p) = queue.pop_front() {
                group_hash ^= Self::stone_hash(p, color);
                let (neighbors, neighbor_count) = self.neighbors(p);
                for &neighbor in &neighbors[..neighbor_count] {
                    if self.cells[neighbor] == 0 {
                        if liberty_seen[neighbor] != marker {
                            liberty_seen[neighbor] = marker;
                            count += 1;
                        }
                    } else if self.cells[neighbor] == color && group_at[neighbor] == usize::MAX {
                        group_at[neighbor] = group;
                        queue.push_back(neighbor);
                    }
                }
            }
            liberties.push(count);
            group_hashes.push(group_hash);
        }
        (group_at, liberties, group_hashes)
    }
    fn has_liberty_after_play(
        &self,
        action: usize,
        group_at: &[usize],
        liberties: &[usize],
    ) -> bool {
        let (neighbors, count) = self.neighbors(action);
        for &neighbor in &neighbors[..count] {
            match self.cells[neighbor] {
                0 => return true,
                color if color == self.turn => {
                    if liberties[group_at[neighbor]] > 1 {
                        return true;
                    }
                }
                _ => {
                    if liberties[group_at[neighbor]] == 1 {
                        return true;
                    }
                }
            }
        }
        false
    }
    fn stone_hash(point: usize, color: i8) -> u64 {
        let mut value = (point as u64)
            .wrapping_mul(2)
            .wrapping_add(u64::from(color == -1))
            .wrapping_add(0x9e3779b97f4a7c15);
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
        value ^ (value >> 31)
    }
    fn position_hash(&self) -> u64 {
        self.cells
            .iter()
            .enumerate()
            .filter(|(_, color)| **color != 0)
            .fold(0, |hash, (point, &color)| {
                hash ^ Self::stone_hash(point, color)
            })
    }
    fn position_hash_after_play(
        &self,
        action: usize,
        group_at: &[usize],
        liberties: &[usize],
        group_hashes: &[u64],
        current_hash: u64,
    ) -> u64 {
        let (neighbors, count) = self.neighbors(action);
        let mut hash = current_hash ^ Self::stone_hash(action, self.turn);
        let mut captured_groups = Vec::new();
        for &neighbor in &neighbors[..count] {
            if self.cells[neighbor] == -self.turn {
                let group = group_at[neighbor];
                if liberties[group] == 1 && !captured_groups.contains(&group) {
                    captured_groups.push(group);
                }
            }
        }
        for group in captured_groups {
            hash ^= group_hashes[group];
        }
        hash
    }
    pub fn play(&mut self, action: usize) -> Result<(), String> {
        self.play_inner(action, true)
    }
    pub(crate) fn play_for_analysis(&mut self, action: usize) -> Result<(), String> {
        self.play_inner(action, false)
    }
    fn play_inner(&mut self, action: usize, enforce_superko: bool) -> Result<(), String> {
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
        let (neighbors, count) = self.neighbors(action);
        let next_hash = if enforce_superko {
            let (group_at, liberties, group_hashes) = self.group_liberties();
            if !self.has_liberty_after_play(action, &group_at, &liberties) {
                return Err("illegal suicide".into());
            }
            Some(self.position_hash_after_play(
                action,
                &group_at,
                &liberties,
                &group_hashes,
                self.position_hash(),
            ))
        } else {
            let mut has_liberty = false;
            for &neighbor in &neighbors[..count] {
                if self.cells[neighbor] == 0 {
                    has_liberty = true;
                    break;
                }
                let (_, liberties) = self.group(neighbor);
                if (self.cells[neighbor] == self.turn && liberties > 1)
                    || (self.cells[neighbor] == -self.turn && liberties == 1)
                {
                    has_liberty = true;
                    break;
                }
            }
            if !has_liberty {
                return Err("illegal suicide".into());
            }
            None
        };
        self.cells[action] = self.turn;
        let mut captured = Vec::new();
        for &n in &neighbors[..count] {
            if self.cells[n] == -self.turn {
                let (stones, remaining_liberties) = self.group(n);
                if remaining_liberties == 0 {
                    for point in stones {
                        self.cells[point] = 0;
                        captured.push(point);
                    }
                }
            }
        }
        if next_hash.is_some_and(|hash| self.position_history.contains(&hash)) {
            self.cells[action] = 0;
            for point in captured {
                self.cells[point] = -self.turn;
            }
            return Err("illegal positional superko".into());
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
        if let Some(hash) = next_hash {
            Arc::make_mut(&mut self.position_history).insert(hash);
        }
        Ok(())
    }
    pub fn legal_moves(&self) -> Vec<usize> {
        let mut out = Vec::new();
        let (group_at, liberties, group_hashes) = self.group_liberties();
        let current_hash = self.position_hash();
        for p in 0..self.cells.len() {
            if self.cells[p] == 0
                && self.ko != Some(p)
                && self.has_liberty_after_play(p, &group_at, &liberties)
                && !self
                    .position_history
                    .contains(&self.position_hash_after_play(
                        p,
                        &group_at,
                        &liberties,
                        &group_hashes,
                        current_hash,
                    ))
            {
                out.push(p);
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
                        let (neighbors, count) = self.neighbors(x);
                        for &n in &neighbors[..count] {
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

    pub fn features_with_komi(&self, komi: f32) -> Vec<f32> {
        let mut features = self.features();
        let turn_komi = if self.turn == 1 { -komi } else { komi };
        features.push(turn_komi / self.cells.len() as f32);
        features
    }
}

#[derive(Clone)]
pub struct GoGame {
    pub size: usize,
    pub moves: Vec<usize>,
    pub winner: i8,
    pub result: Option<String>,
    pub komi: Option<String>,
    pub first_turn: i8,
    pub black_setup: Vec<usize>,
    pub white_setup: Vec<usize>,
}

impl GoGame {
    pub fn komi_value(&self) -> Result<f32, String> {
        let komi = match &self.komi {
            Some(value) => value.parse::<f32>().map_err(|_| "invalid Go komi")?,
            None => 6.5,
        };
        if !komi.is_finite() {
            return Err("invalid Go komi".into());
        }
        Ok(komi)
    }
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
        let score = b.score_black(6.5);
        let winner = if score > 0.0 {
            1
        } else if score < 0.0 {
            -1
        } else {
            0
        };
        let result = if score == 0.0 {
            "0".to_owned()
        } else if score > 0.0 {
            format!("B+{score:.1}")
        } else {
            format!("W+{:.1}", score.abs())
        };
        let moves = seq
            .iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>()
            .join(" ");
        writeln!(w, "{size},{winner},1,-,-,{moves},{result},6.5").map_err(|e| e.to_string())?;
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
        n.play_for_analysis(a).unwrap();
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
        if line.trim().is_empty() {
            continue;
        }
        let fields: Vec<_> = line.splitn(8, ',').collect();
        let size = fields
            .first()
            .ok_or("bad Go row")?
            .parse()
            .map_err(|_| "bad size")?;
        let winner = fields
            .get(1)
            .ok_or("bad Go row")?
            .parse()
            .map_err(|_| "bad winner")?;
        let (first_turn, black_setup, white_setup, moves_field) = if fields.len() == 3 {
            (1, Vec::new(), Vec::new(), fields[2])
        } else if (6..=8).contains(&fields.len()) {
            (
                fields[2].parse().map_err(|_| "bad Go turn")?,
                parse_points(fields[3], size)?,
                parse_points(fields[4], size)?,
                fields[5],
            )
        } else {
            return Err("bad Go row".into());
        };
        let moves = moves_field
            .split_whitespace()
            .map(|x| x.parse().map_err(|_| "bad move"))
            .collect::<Result<_, _>>()?;
        GoBoard::with_setup(size, &black_setup, &white_setup, first_turn)?;
        out.push(GoGame {
            size,
            moves,
            winner,
            result: fields
                .get(6)
                .filter(|result| **result != "-")
                .map(|result| (*result).to_owned()),
            komi: fields
                .get(7)
                .filter(|komi| **komi != "-")
                .map(|komi| (*komi).to_owned()),
            first_turn,
            black_setup,
            white_setup,
        });
        validate_game(out.last().ok_or("missing loaded Go game")?)?;
    }
    Ok(out)
}

pub fn validate_game(game: &GoGame) -> Result<(), String> {
    if !matches!(game.winner, -1..=1) {
        return Err("Go winner must be -1, 0, or 1".into());
    }
    game.komi_value()?;
    let mut board = GoBoard::with_setup(
        game.size,
        &game.black_setup,
        &game.white_setup,
        game.first_turn,
    )?;
    if let Some(result) = &game.result {
        let result_winner = match result.as_str() {
            "0" | "Draw" | "Jigo" => Some(0),
            value if value.starts_with("B+") => Some(1),
            value if value.starts_with("W+") => Some(-1),
            _ => None,
        };
        if result_winner.is_some_and(|result_winner| result_winner != game.winner) {
            return Err("Go winner does not match recorded result".into());
        }
    }
    for (ply, &action) in game.moves.iter().enumerate() {
        if board.terminal() {
            return Err(format!("Go move after game ended at ply {}", ply + 1));
        }
        board
            .play(action)
            .map_err(|error| format!("illegal Go move at ply {}: {error}", ply + 1))?;
    }
    Ok(())
}

pub fn split_games(
    games: &[GoGame],
    size: usize,
    validation_fraction: f32,
    seed: u64,
) -> Result<(Vec<GoGame>, Vec<GoGame>), String> {
    if !validation_fraction.is_finite() || !(0.0..1.0).contains(&validation_fraction) {
        return Err("Go validation fraction must be in [0, 1)".into());
    }

    let mut indices: Vec<_> = games
        .iter()
        .enumerate()
        .filter_map(|(index, game)| (game.size == size).then_some(index))
        .collect();
    if indices.is_empty() {
        return Err(format!("no Go games for board size {size}"));
    }

    let validation_count = if validation_fraction == 0.0 {
        0
    } else {
        if indices.len() < 2 {
            return Err("Go validation split requires at least two games".into());
        }
        ((indices.len() as f32 * validation_fraction).round() as usize).clamp(1, indices.len() - 1)
    };
    Rng::new(seed).shuffle(&mut indices);

    let validation = indices[..validation_count]
        .iter()
        .map(|&index| games[index].clone())
        .collect();
    let training = indices[validation_count..]
        .iter()
        .map(|&index| games[index].clone())
        .collect();
    Ok((training, validation))
}

fn parse_points(value: &str, size: usize) -> Result<Vec<usize>, String> {
    if value == "-" || value.is_empty() {
        return Ok(Vec::new());
    }
    value
        .split(':')
        .map(|point| {
            let point = point.parse::<usize>().map_err(|_| "bad setup point")?;
            if point >= size * size {
                return Err("setup point outside board".into());
            }
            Ok(point)
        })
        .collect()
}

#[derive(Default)]
struct SgfNode {
    properties: Vec<(String, Vec<String>)>,
}

struct SgfTree {
    sequence: Vec<SgfNode>,
    variations: Vec<SgfTree>,
}

struct SgfParser<'a> {
    input: &'a [u8],
    cursor: usize,
}

impl<'a> SgfParser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input: input.as_bytes(),
            cursor: 0,
        }
    }

    fn skip_whitespace(&mut self) {
        while self
            .input
            .get(self.cursor)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.cursor += 1;
        }
    }

    fn parse_collection(&mut self) -> Result<Vec<SgfTree>, String> {
        let mut trees = Vec::new();
        self.skip_whitespace();
        while self.cursor < self.input.len() {
            trees.push(self.parse_tree()?);
            self.skip_whitespace();
        }
        if trees.is_empty() {
            return Err("SGF contains no game trees".into());
        }
        Ok(trees)
    }

    fn parse_tree(&mut self) -> Result<SgfTree, String> {
        self.expect(b'(')?;
        self.skip_whitespace();
        let mut sequence = Vec::new();
        while self.peek() == Some(b';') {
            sequence.push(self.parse_node()?);
            self.skip_whitespace();
        }
        if sequence.is_empty() {
            return Err("SGF game tree has no nodes".into());
        }
        let mut variations = Vec::new();
        while self.peek() == Some(b'(') {
            variations.push(self.parse_tree()?);
            self.skip_whitespace();
        }
        self.expect(b')')?;
        Ok(SgfTree {
            sequence,
            variations,
        })
    }

    fn parse_node(&mut self) -> Result<SgfNode, String> {
        self.expect(b';')?;
        let mut node = SgfNode::default();
        loop {
            self.skip_whitespace();
            if !self.peek().is_some_and(|byte| byte.is_ascii_uppercase()) {
                break;
            }
            let start = self.cursor;
            while self.peek().is_some_and(|byte| byte.is_ascii_uppercase()) {
                self.cursor += 1;
            }
            let property = std::str::from_utf8(&self.input[start..self.cursor])
                .map_err(|_| "invalid SGF property")?
                .to_owned();
            let mut values = Vec::new();
            loop {
                self.skip_whitespace();
                if self.peek() != Some(b'[') {
                    break;
                }
                values.push(self.parse_value()?);
            }
            if values.is_empty() {
                return Err(format!("SGF property {property} has no value"));
            }
            node.properties.push((property, values));
        }
        Ok(node)
    }

    fn parse_value(&mut self) -> Result<String, String> {
        self.expect(b'[')?;
        let mut value = Vec::new();
        loop {
            let byte = *self
                .input
                .get(self.cursor)
                .ok_or("unterminated SGF property value")?;
            self.cursor += 1;
            match byte {
                b']' => return String::from_utf8(value).map_err(|_| "invalid UTF-8 in SGF".into()),
                b'\\' => {
                    let escaped = *self
                        .input
                        .get(self.cursor)
                        .ok_or("unterminated SGF escape")?;
                    self.cursor += 1;
                    if escaped == b'\n' {
                        continue;
                    }
                    if escaped == b'\r' {
                        if self.input.get(self.cursor) == Some(&b'\n') {
                            self.cursor += 1;
                        }
                        continue;
                    }
                    value.push(escaped);
                }
                _ => value.push(byte),
            }
        }
    }

    fn peek(&self) -> Option<u8> {
        self.input.get(self.cursor).copied()
    }

    fn expect(&mut self, expected: u8) -> Result<(), String> {
        if self.peek() != Some(expected) {
            return Err(format!(
                "invalid SGF structure at byte {}: expected {}",
                self.cursor, expected as char
            ));
        }
        self.cursor += 1;
        Ok(())
    }
}

fn property_values<'a>(node: &'a SgfNode, name: &str) -> Vec<&'a str> {
    node.properties
        .iter()
        .filter(|(property, _)| property == name)
        .flat_map(|(_, values)| values.iter().map(String::as_str))
        .collect()
}

fn mainline_nodes(tree: &SgfTree) -> Vec<&SgfNode> {
    let mut nodes: Vec<_> = tree.sequence.iter().collect();
    if let Some(first_variation) = tree.variations.first() {
        nodes.extend(mainline_nodes(first_variation));
    }
    nodes
}

fn sgf_point(value: &str, size: usize) -> Result<usize, String> {
    let bytes = value.as_bytes();
    if bytes.len() != 2 || !bytes.iter().all(u8::is_ascii_lowercase) {
        return Err(format!("invalid SGF coordinate: {value}"));
    }
    let col = (bytes[0] - b'a') as usize;
    let row = (bytes[1] - b'a') as usize;
    if row >= size || col >= size {
        return Err(format!(
            "SGF coordinate outside {size}x{size} board: {value}"
        ));
    }
    Ok(row * size + col)
}

fn sgf_setup_points(values: &[&str], size: usize) -> Result<Vec<usize>, String> {
    let mut points = Vec::new();
    for value in values {
        let mut bounds = value.split(':');
        let start = sgf_point(bounds.next().unwrap_or(""), size)?;
        let end = match bounds.next() {
            Some(point) => sgf_point(point, size)?,
            None => start,
        };
        if bounds.next().is_some() {
            return Err(format!("invalid SGF setup range: {value}"));
        }
        let (first_row, last_row) = (start / size, end / size);
        let (first_col, last_col) = (start % size, end % size);
        for row in first_row.min(last_row)..=first_row.max(last_row) {
            for col in first_col.min(last_col)..=first_col.max(last_col) {
                points.push(row * size + col);
            }
        }
    }
    points.sort_unstable();
    points.dedup();
    Ok(points)
}

fn sgf_move(value: &str, size: usize) -> Result<usize, String> {
    if value.is_empty() || value == "tt" {
        return Ok(size * size);
    }
    sgf_point(value, size)
}

fn parse_sgf_game(tree: &SgfTree) -> Result<Option<GoGame>, String> {
    let nodes = mainline_nodes(tree);
    let root = nodes.first().ok_or("empty SGF main line")?;
    if property_values(root, "GM")
        .first()
        .is_some_and(|&game| game != "1")
    {
        return Ok(None);
    }
    let size = property_values(root, "SZ")
        .first()
        .map_or(Ok(19usize), |value| {
            value.parse().map_err(|_| "unsupported SGF board size")
        })?;
    GoBoard::new(size)?;
    let result = property_values(root, "RE").first().copied().unwrap_or("");
    let winner = match result {
        value if value.starts_with("B+") => 1,
        value if value.starts_with("W+") => -1,
        "0" | "Draw" | "Jigo" => 0,
        _ => return Ok(None),
    };
    let komi = match property_values(root, "KM").first() {
        Some(value) => {
            let parsed = value.parse::<f32>().map_err(|_| "invalid SGF komi")?;
            if !parsed.is_finite() {
                return Err("invalid SGF komi".into());
            }
            Some((*value).to_owned())
        }
        None => None,
    };

    let mut black_setup = Vec::new();
    let mut white_setup = Vec::new();
    let mut player_to_move = None;
    let mut first_move_node = nodes.len();
    for (index, node) in nodes.iter().enumerate() {
        let removed = sgf_setup_points(&property_values(node, "AE"), size)?;
        black_setup.retain(|point| !removed.contains(point));
        white_setup.retain(|point| !removed.contains(point));

        let black_added = sgf_setup_points(&property_values(node, "AB"), size)?;
        let white_added = sgf_setup_points(&property_values(node, "AW"), size)?;
        if black_added.iter().any(|point| white_added.contains(point))
            || black_added.iter().any(|point| white_setup.contains(point))
            || white_added.iter().any(|point| black_setup.contains(point))
        {
            return Err("overlapping SGF setup stones".into());
        }
        black_setup.extend(black_added);
        white_setup.extend(white_added);

        if let Some(player) = property_values(node, "PL").first().copied() {
            player_to_move = Some(match player {
                "B" => 1,
                "W" => -1,
                _ => return Err("invalid SGF player-to-move property".into()),
            });
        }
        if !property_values(node, "B").is_empty() || !property_values(node, "W").is_empty() {
            first_move_node = index;
            break;
        }
    }
    black_setup.sort_unstable();
    black_setup.dedup();
    white_setup.sort_unstable();
    white_setup.dedup();
    let first_turn = player_to_move.unwrap_or(if black_setup.len() > 1 { -1 } else { 1 });
    let mut board = GoBoard::with_setup(size, &black_setup, &white_setup, first_turn)?;
    let mut moves = Vec::new();
    for node in nodes.into_iter().skip(first_move_node) {
        let black_moves = property_values(node, "B");
        let white_moves = property_values(node, "W");
        if black_moves.is_empty() && white_moves.is_empty() {
            continue;
        }
        if black_moves.len() + white_moves.len() != 1 {
            return Err("SGF node must contain exactly one move".into());
        }
        let (color, value) = if let Some(value) = black_moves.first() {
            (1, *value)
        } else {
            (-1, white_moves[0])
        };
        if board.turn != color {
            return Err(format!(
                "SGF move color mismatch at ply {}",
                moves.len() + 1
            ));
        }
        let action = sgf_move(value, size)?;
        board
            .play(action)
            .map_err(|error| format!("illegal SGF move at ply {}: {error}", moves.len() + 1))?;
        moves.push(action);
    }
    Ok(Some(GoGame {
        size,
        moves,
        winner,
        result: Some(result.to_owned()),
        komi,
        first_turn,
        black_setup,
        white_setup,
    }))
}

#[derive(Debug, Default)]
pub struct SgfImportStats {
    pub imported: usize,
    pub skipped: usize,
    pub invalid: usize,
    pub invalid_examples: Vec<String>,
}

fn collect_sgf_files(path: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    if path.is_file() {
        files.push(path.to_path_buf());
        return Ok(());
    }
    for entry in fs::read_dir(path).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let entry_path = entry.path();
        if entry_path.is_dir() {
            collect_sgf_files(&entry_path, files)?;
        } else if entry_path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("sgf"))
        {
            files.push(entry_path);
        }
    }
    Ok(())
}

pub fn import_sgf(input: &str, output: &str) -> Result<SgfImportStats, String> {
    import_sgf_with_options(input, output, false)
}

pub fn import_sgf_with_options(
    input: &str,
    output: &str,
    skip_invalid: bool,
) -> Result<SgfImportStats, String> {
    let input_path = Path::new(input);
    let mut files = Vec::new();
    collect_sgf_files(input_path, &mut files)?;
    files.sort();
    if files.is_empty() {
        return Err("no SGF files found".into());
    }

    let mut games = Vec::new();
    let mut stats = SgfImportStats::default();
    for path in files {
        let bytes = fs::read(&path).map_err(|error| error.to_string())?;
        let text = String::from_utf8_lossy(&bytes);
        let trees = match SgfParser::new(&text).parse_collection() {
            Ok(trees) => trees,
            Err(error) if skip_invalid => {
                stats.invalid += 1;
                if stats.invalid_examples.len() < 5 {
                    stats
                        .invalid_examples
                        .push(format!("{}: {error}", path.display()));
                }
                continue;
            }
            Err(error) => return Err(format!("{}: {error}", path.display())),
        };
        for tree in trees {
            match parse_sgf_game(&tree) {
                Err(error) if skip_invalid => {
                    stats.invalid += 1;
                    if stats.invalid_examples.len() < 5 {
                        stats
                            .invalid_examples
                            .push(format!("{}: {error}", path.display()));
                    }
                }
                Err(error) => return Err(format!("{}: {error}", path.display())),
                Ok(Some(game)) => {
                    games.push(game);
                    stats.imported += 1;
                }
                Ok(None) => stats.skipped += 1,
            }
        }
    }
    save_games(output, &games)?;
    Ok(stats)
}

pub fn save_games(path: &str, games: &[GoGame]) -> Result<(), String> {
    let mut writer = BufWriter::new(File::create(path).map_err(|error| error.to_string())?);
    for game in games {
        let black_setup = serialize_points(&game.black_setup);
        let white_setup = serialize_points(&game.white_setup);
        let moves = game
            .moves
            .iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join(" ");
        writeln!(
            writer,
            "{},{},{},{},{},{},{},{}",
            game.size,
            game.winner,
            game.first_turn,
            black_setup,
            white_setup,
            moves,
            game.result.as_deref().unwrap_or("-"),
            game.komi.as_deref().unwrap_or("-")
        )
        .map_err(|error| error.to_string())?;
    }
    Ok(())
}

pub fn export_sgf(input: &str, output: &str) -> Result<usize, String> {
    let games = load_games(input)?;
    let mut writer = BufWriter::new(File::create(output).map_err(|error| error.to_string())?);
    for (index, game) in games.iter().enumerate() {
        let result = game
            .result
            .as_deref()
            .ok_or_else(|| format!("game {} has no SGF result metadata", index + 1))?;
        let mut board = GoBoard::with_setup(
            game.size,
            &game.black_setup,
            &game.white_setup,
            game.first_turn,
        )?;
        write!(writer, "(;GM[1]FF[4]CA[UTF-8]SZ[{}]", game.size)
            .map_err(|error| error.to_string())?;
        if let Some(komi) = &game.komi {
            write!(writer, "KM[{}]", escape_sgf_value(komi)).map_err(|error| error.to_string())?;
        }
        write!(writer, "RE[{}]", escape_sgf_value(result)).map_err(|error| error.to_string())?;
        for &point in &game.black_setup {
            write!(writer, "AB[{}]", sgf_coordinate(point, game.size))
                .map_err(|error| error.to_string())?;
        }
        for &point in &game.white_setup {
            write!(writer, "AW[{}]", sgf_coordinate(point, game.size))
                .map_err(|error| error.to_string())?;
        }
        if game.first_turn == -1 {
            write!(writer, "PL[W]").map_err(|error| error.to_string())?;
        }
        for &action in &game.moves {
            let color = if board.turn == 1 { 'B' } else { 'W' };
            if action == board.pass_action() {
                write!(writer, ";{color}[]").map_err(|error| error.to_string())?;
            } else {
                write!(writer, ";{color}[{}]", sgf_coordinate(action, game.size))
                    .map_err(|error| error.to_string())?;
            }
            board
                .play(action)
                .map_err(|error| format!("game {} contains an illegal move: {error}", index + 1))?;
        }
        writeln!(writer, ")").map_err(|error| error.to_string())?;
    }
    Ok(games.len())
}

fn sgf_coordinate(point: usize, size: usize) -> String {
    let col = (point % size) as u8;
    let row = (point / size) as u8;
    String::from_utf8(vec![b'a' + col, b'a' + row]).expect("ASCII SGF coordinate")
}

fn escape_sgf_value(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        if matches!(ch, '\\' | ']') {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    escaped
}

fn serialize_points(points: &[usize]) -> String {
    if points.is_empty() {
        "-".into()
    } else {
        points
            .iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join(":")
    }
}

#[derive(Clone)]
pub struct GoModel {
    pub size: usize,
    pub value: Vec<f32>,
    pub policy: Vec<f32>,
}
impl GoModel {
    pub fn new(size: usize) -> Self {
        let f = size * size * 2 + 2;
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
        self.value_with_komi(b, 6.5)
    }
    pub fn value_with_komi(&self, b: &GoBoard, komi: f32) -> f32 {
        Self::dot(&self.value, &b.features_with_komi(komi)).tanh()
    }
    pub fn choose(&self, b: &GoBoard, rng: &mut Rng) -> usize {
        self.choose_with_komi(b, rng, 6.5)
    }
    pub fn choose_with_komi(&self, b: &GoBoard, rng: &mut Rng, komi: f32) -> usize {
        let x = b.features_with_komi(komi);
        let f = x.len();
        let mut scored = Vec::new();
        for a in b.legal_moves() {
            let mut next = b.clone();
            next.play_for_analysis(a).unwrap();
            let s =
                Self::dot(&self.policy[a * f..(a + 1) * f], &x) - self.value_with_komi(&next, komi);
            scored.push((s, rng.next_u64(), a));
        }
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap().then_with(|| b.1.cmp(&a.1)));
        scored[0].2
    }
    pub fn evaluate_loss(&self, games: &[GoGame]) -> Result<(f32, usize), String> {
        let mut loss = 0.0;
        let mut moves = 0;
        for game in games.iter().filter(|game| game.size == self.size) {
            validate_game(game)?;
            let komi = game.komi_value()?;
            let mut board = GoBoard::with_setup(
                self.size,
                &game.black_setup,
                &game.white_setup,
                game.first_turn,
            )?;
            for &action in &game.moves {
                let target = match game.winner {
                    0 => 0.0,
                    winner if winner == board.turn => 1.0,
                    _ => -1.0,
                };
                let error = target - self.value_with_komi(&board, komi);
                loss += error * error;
                moves += 1;
                board.play(action)?;
            }
        }
        if moves == 0 {
            return Err("Go evaluation set contains no moves for this board size".into());
        }
        Ok((loss / moves as f32, moves))
    }
    pub fn evaluate_policy_top1(&self, games: &[GoGame]) -> Result<(f32, f32, usize), String> {
        let mut correct = 0.0;
        let mut random_correct = 0.0;
        let mut moves = 0;
        let features = self.value.len();
        for game in games.iter().filter(|game| game.size == self.size) {
            validate_game(game)?;
            let komi = game.komi_value()?;
            let mut board = GoBoard::with_setup(
                self.size,
                &game.black_setup,
                &game.white_setup,
                game.first_turn,
            )?;
            for &action in &game.moves {
                let input = board.features_with_komi(komi);
                let legal = board.legal_moves();
                let mut best_score = f32::NEG_INFINITY;
                let mut tied_best = 0;
                let mut chosen_score = f32::NEG_INFINITY;
                for candidate in legal.iter().copied() {
                    let score = Self::dot(
                        &self.policy[candidate * features..(candidate + 1) * features],
                        &input,
                    );
                    if candidate == action {
                        chosen_score = score;
                    }
                    match score.total_cmp(&best_score) {
                        std::cmp::Ordering::Greater => {
                            best_score = score;
                            tied_best = 1;
                        }
                        std::cmp::Ordering::Equal => tied_best += 1,
                        std::cmp::Ordering::Less => {}
                    }
                }
                if chosen_score.total_cmp(&best_score) == std::cmp::Ordering::Equal {
                    correct += 1.0 / tied_best as f32;
                }
                random_correct += 1.0 / legal.len() as f32;
                moves += 1;
                board.play(action)?;
            }
        }
        if moves == 0 {
            return Err("Go policy evaluation set contains no moves for this board size".into());
        }
        Ok((correct / moves as f32, random_correct / moves as f32, moves))
    }
    pub fn train_with_validation(
        &mut self,
        training_games: &[GoGame],
        validation_games: &[GoGame],
        epochs: usize,
        seed: u64,
    ) -> Result<(usize, f32, f32), String> {
        let (mut best_loss, _) = self.evaluate_loss(validation_games)?;
        let mut best_model = self.clone();
        let mut best_epoch = 0;
        let mut best_train_loss = 0.0;
        for epoch in 0..epochs {
            let train_loss = self.train(training_games, 1, seed.wrapping_add(epoch as u64))?;
            let (validation_loss, _) = self.evaluate_loss(validation_games)?;
            if validation_loss < best_loss {
                best_loss = validation_loss;
                best_epoch = epoch + 1;
                best_train_loss = train_loss;
                best_model = self.clone();
            }
        }
        *self = best_model;
        let train_mse = if best_epoch == 0 {
            best_train_loss
        } else {
            self.evaluate_loss(training_games)?.0
        };
        Ok((best_epoch, train_mse, best_loss))
    }
    pub fn train(&mut self, games: &[GoGame], epochs: usize, seed: u64) -> Result<f32, String> {
        let mut rng = Rng::new(seed);
        let f = self.value.len();
        let mut loss = 0.0;
        for game in games.iter().filter(|game| game.size == self.size) {
            validate_game(game)?;
            game.komi_value()?;
        }
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
                let komi = game.komi_value()?;
                let mut b = GoBoard::with_setup(
                    self.size,
                    &game.black_setup,
                    &game.white_setup,
                    game.first_turn,
                )?;
                for &action in &game.moves {
                    let x = b.features_with_komi(komi);
                    let target = match game.winner {
                        0 => 0.0,
                        winner if winner == b.turn => 1.0,
                        _ => -1.0,
                    };
                    let pred = self.value_with_komi(&b, komi);
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
                            self.value[j] += 0.00001 * err * (1.0 - pred * pred) * x[j];
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
        writeln!(w, "NGO2 {}", self.size).map_err(|e| e.to_string())?;
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
        let mut header = h.split_whitespace();
        let version = header.next().ok_or("bad Go model")?;
        let size = header
            .next()
            .ok_or("bad Go model")?
            .parse()
            .map_err(|_| "bad size")?;
        if header.next().is_some() || !matches!(version, "NGO1" | "NGO2") {
            return Err("unsupported Go model".into());
        }
        let mut m = Self::new(size);
        let vals = lines
            .map(|l| {
                l.map_err(|e| e.to_string())?
                    .parse()
                    .map_err(|_| "bad weight".to_string())
            })
            .collect::<Result<Vec<f32>, _>>()?;
        let f = m.value.len();
        let legacy_features = f - 1;
        if version == "NGO1" && vals.len() == legacy_features + (size * size + 1) * legacy_features
        {
            for (target, source) in m.value.iter_mut().zip(&vals[..legacy_features]) {
                *target = *source;
            }
            let old_policy = &vals[legacy_features..];
            for action in 0..=size * size {
                let new_start = action * f;
                let old_start = action * legacy_features;
                m.policy[new_start..new_start + legacy_features]
                    .copy_from_slice(&old_policy[old_start..old_start + legacy_features]);
            }
            return Ok(m);
        }
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
    benchmark_with_komi(model, games, seed, heuristic, 6.5)
}

pub fn benchmark_with_komi(
    model: &GoModel,
    games: usize,
    seed: u64,
    heuristic: bool,
    komi: f32,
) -> Result<(usize, usize), String> {
    if !komi.is_finite() {
        return Err("invalid Go komi".into());
    }
    let mut rng = Rng::new(seed);
    let mut wins = 0;
    for g in 0..games {
        let model_black = g % 2 == 0;
        let mut board = GoBoard::new(model.size)?;
        while !board.terminal() {
            let model_turn = (board.turn == 1) == model_black;
            let action = if model_turn {
                model.choose_with_komi(&board, &mut rng, komi)
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
        let black_win = board.score_black(komi) > 0.0;
        if black_win == model_black {
            wins += 1;
        }
    }
    Ok((wins, games - wins))
}

fn gtp_query(
    input: &mut BufWriter<ChildStdin>,
    output: &mut BufReader<ChildStdout>,
    command: &str,
) -> Result<String, String> {
    writeln!(input, "{command}").map_err(|error| error.to_string())?;
    input.flush().map_err(|error| error.to_string())?;

    let mut response = Vec::new();
    loop {
        let mut line = String::new();
        if output
            .read_line(&mut line)
            .map_err(|error| error.to_string())?
            == 0
        {
            return Err(format!("GTP engine closed stdout during `{command}`"));
        }
        if line.trim().is_empty() {
            break;
        }
        response.push(line);
    }

    let response = response.join("").trim().to_owned();
    if let Some(error) = response.strip_prefix('?') {
        return Err(format!("GTP engine rejected `{command}`: {}", error.trim()));
    }
    response
        .strip_prefix('=')
        .map(|payload| payload.trim().to_owned())
        .ok_or_else(|| format!("invalid GTP response to `{command}`: {response}"))
}

fn gtp_color(color: i8) -> &'static str {
    if color == 1 {
        "black"
    } else {
        "white"
    }
}

fn gtp_vertex(action: usize, board: &GoBoard) -> String {
    if action == board.pass_action() {
        return "pass".into();
    }
    let mut column = b'A' + (action % board.size) as u8;
    if column >= b'I' {
        column += 1;
    }
    let row = board.size - action / board.size;
    format!("{}{}", column as char, row)
}

fn parse_gtp_vertex(vertex: &str, board: &GoBoard) -> Result<Option<usize>, String> {
    if vertex.eq_ignore_ascii_case("pass") {
        return Ok(Some(board.pass_action()));
    }
    if vertex.eq_ignore_ascii_case("resign") {
        return Ok(None);
    }
    let bytes = vertex.as_bytes();
    if bytes.len() < 2 || !bytes[0].is_ascii_alphabetic() {
        return Err(format!("invalid GTP move: {vertex}"));
    }
    let column = bytes[0].to_ascii_uppercase();
    if column == b'I' {
        return Err(format!("invalid GTP column: {vertex}"));
    }
    let mut col = (column - b'A') as usize;
    if column > b'I' {
        col -= 1;
    }
    let row = std::str::from_utf8(&bytes[1..])
        .map_err(|_| format!("invalid GTP row: {vertex}"))?
        .parse::<usize>()
        .map_err(|_| format!("invalid GTP row: {vertex}"))?;
    if col >= board.size || !(1..=board.size).contains(&row) {
        return Err(format!(
            "GTP move outside {}x{} board: {vertex}",
            board.size, board.size
        ));
    }
    Ok(Some((board.size - row) * board.size + col))
}

pub fn benchmark_gtp(
    model: &GoModel,
    engine: &str,
    engine_args: &[String],
    games: usize,
    seed: u64,
    komi: f32,
) -> Result<GtpBenchmarkResults, String> {
    if games == 0 {
        return Err("GTP benchmark requires at least one game".into());
    }
    if !komi.is_finite() {
        return Err("invalid Go komi".into());
    }
    let mut child = Command::new(engine)
        .args(engine_args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|error| format!("failed to start GTP engine `{engine}`: {error}"))?;
    let mut input = BufWriter::new(child.stdin.take().ok_or("missing GTP engine stdin")?);
    let mut output = BufReader::new(child.stdout.take().ok_or("missing GTP engine stdout")?);
    let result = (|| {
        let mut rng = Rng::new(seed);
        let mut results = GtpBenchmarkResults::default();
        for game_index in 0..games {
            gtp_query(
                &mut input,
                &mut output,
                &format!("boardsize {}", model.size),
            )?;
            gtp_query(&mut input, &mut output, "clear_board")?;
            gtp_query(&mut input, &mut output, &format!("komi {komi}"))?;
            let model_color = if game_index % 2 == 0 { 1 } else { -1 };
            let mut board = GoBoard::new(model.size)?;
            let mut resigned = false;
            while !board.terminal() {
                if board.turn == model_color {
                    let action = model.choose_with_komi(&board, &mut rng, komi);
                    gtp_query(
                        &mut input,
                        &mut output,
                        &format!(
                            "play {} {}",
                            gtp_color(board.turn),
                            gtp_vertex(action, &board)
                        ),
                    )?;
                    board.play(action)?;
                } else {
                    let vertex = gtp_query(
                        &mut input,
                        &mut output,
                        &format!("genmove {}", gtp_color(board.turn)),
                    )?;
                    let Some(action) = parse_gtp_vertex(&vertex, &board)? else {
                        resigned = true;
                        break;
                    };
                    if !board.legal_moves().contains(&action) {
                        return Err(format!(
                            "GTP engine returned illegal move {vertex} in game {}",
                            game_index + 1
                        ));
                    }
                    board.play(action)?;
                }
            }
            if resigned {
                results.record(model_color, Some(true));
            } else {
                let score = board.score_black(komi);
                let model_score = score * model_color as f32;
                if model_score > 0.0 {
                    results.record(model_color, Some(true));
                } else if model_score < 0.0 {
                    results.record(model_color, Some(false));
                } else {
                    results.record(model_color, None);
                }
            }
        }
        Ok(results)
    })();

    match result {
        Ok(stats) => {
            let _ = gtp_query(&mut input, &mut output, "quit");
            child.wait().map_err(|error| error.to_string())?;
            Ok(stats)
        }
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            Err(error)
        }
    }
}

#[derive(Default)]
pub struct GtpBenchmarkResults {
    pub wins: usize,
    pub losses: usize,
    pub draws: usize,
    pub model_black: (usize, usize, usize),
    pub model_white: (usize, usize, usize),
}

impl GtpBenchmarkResults {
    fn record(&mut self, model_color: i8, win: Option<bool>) {
        let color_results = if model_color == 1 {
            &mut self.model_black
        } else {
            &mut self.model_white
        };
        match win {
            Some(true) => {
                self.wins += 1;
                color_results.0 += 1;
            }
            Some(false) => {
                self.losses += 1;
                color_results.1 += 1;
            }
            None => {
                self.draws += 1;
                color_results.2 += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gtp_benchmark_results_track_outcomes_by_color() {
        let mut results = GtpBenchmarkResults::default();
        results.record(1, Some(true));
        results.record(-1, Some(false));
        results.record(-1, None);

        assert_eq!((results.wins, results.losses, results.draws), (1, 1, 1));
        assert_eq!(results.model_black, (1, 0, 0));
        assert_eq!(results.model_white, (0, 1, 1));
    }

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

    #[test]
    fn optimized_legal_moves_match_play_validation() {
        let mut rng = Rng::new(42);
        for size in [5, 9, 19] {
            let mut board = GoBoard::new(size).unwrap();
            for _ in 0..100 {
                let expected: Vec<_> = (0..board.cells.len())
                    .filter(|&action| {
                        if board.cells[action] != 0 {
                            return false;
                        }
                        let mut next = board.clone();
                        next.play(action).is_ok()
                    })
                    .chain(std::iter::once(board.pass_action()))
                    .collect();
                assert_eq!(board.legal_moves(), expected, "board size {size}");
                if board.terminal() {
                    break;
                }
                let moves = &expected[..expected.len() - 1];
                let action = if moves.is_empty() {
                    board.pass_action()
                } else {
                    moves[rng.usize(moves.len())]
                };
                board.play(action).unwrap();
            }
        }
    }

    #[test]
    fn illegal_suicide_does_not_change_board() {
        let mut board = GoBoard::new(5).unwrap();
        for point in [7, 11, 13, 17] {
            board.cells[point] = -1;
        }
        let before = board.clone();
        assert_eq!(board.play(12), Err("illegal suicide".into()));
        assert_eq!(board.cells, before.cells);
        assert_eq!(board.turn, before.turn);
        assert_eq!(board.moves, before.moves);
    }

    #[test]
    fn positional_superko_rejects_repeated_positions() {
        let mut board = GoBoard::new(5).unwrap();
        let action = 12;
        let (groups, liberties, group_hashes) = board.group_liberties();
        let repeated = board.position_hash_after_play(
            action,
            &groups,
            &liberties,
            &group_hashes,
            board.position_hash(),
        );
        Arc::make_mut(&mut board.position_history).insert(repeated);
        let before = board.cells.clone();

        assert_eq!(board.play(action), Err("illegal positional superko".into()));
        assert!(!board.legal_moves().contains(&action));
        assert_eq!(board.cells, before);
        assert_eq!(board.turn, 1);
        assert_eq!(board.moves, 0);
    }

    #[test]
    fn pass_is_allowed_without_recording_a_repeated_position() {
        let mut board = GoBoard::new(5).unwrap();
        let initial_history_len = board.position_history.len();
        board.play(board.pass_action()).unwrap();

        assert_eq!(board.position_history.len(), initial_history_len);
        assert!(board.legal_moves().contains(&board.pass_action()));
    }

    #[test]
    fn sgf_import_handles_setup_passes_escapes_and_mainline() {
        let sgf = r"(;GM[1]FF[4]SZ[5]KM[0.50]RE[W+R]AB[aa:bb]PL[W]C[escaped \] bracket];W[cc];B[];W[dd](;B[ee])(;B[de]))";
        let trees = SgfParser::new(sgf).parse_collection().unwrap();
        let game = parse_sgf_game(&trees[0]).unwrap().unwrap();

        assert_eq!(game.size, 5);
        assert_eq!(game.winner, -1);
        assert_eq!(game.komi.as_deref(), Some("0.50"));
        assert_eq!(game.first_turn, -1);
        assert_eq!(game.black_setup, vec![0, 1, 5, 6]);
        assert_eq!(game.moves, vec![12, 25, 18, 24]);
    }

    #[test]
    fn sgf_import_applies_handicap_setup_node_before_first_move() {
        let sgf = "(;GM[1]FF[4]SZ[19]HA[4]RE[W+R];AB[pd][dp][dd][pp];W[cq];B[dq])";
        let trees = SgfParser::new(sgf).parse_collection().unwrap();
        let game = parse_sgf_game(&trees[0]).unwrap().unwrap();

        assert_eq!(game.first_turn, -1);
        assert_eq!(game.black_setup, vec![60, 72, 288, 300]);
        assert_eq!(game.moves, vec![306, 307]);
    }

    #[test]
    fn sgf_import_preserves_moves_with_invalid_utf8_metadata() {
        let base = std::env::temp_dir().join(format!("neurofour-sgf-utf8-{}", std::process::id()));
        let sgf = base.with_extension("sgf");
        let csv = base.with_extension("csv");
        std::fs::write(&sgf, b"(;GM[1]FF[4]SZ[5]RE[B+R]PB[\xff];B[aa];W[bb])").unwrap();

        let stats = import_sgf(sgf.to_str().unwrap(), csv.to_str().unwrap()).unwrap();
        let games = load_games(csv.to_str().unwrap()).unwrap();
        std::fs::remove_file(sgf).unwrap();
        std::fs::remove_file(csv).unwrap();

        assert_eq!(stats.imported, 1);
        assert_eq!(games[0].moves, vec![0, 6]);
    }

    #[test]
    fn sgf_import_can_explicitly_skip_invalid_games_and_report_them() {
        let base = std::env::temp_dir().join(format!("neurofour-sgf-skip-{}", std::process::id()));
        let sgf = base.with_extension("sgf");
        let csv = base.with_extension("csv");
        std::fs::write(
            &sgf,
            "(;GM[1]FF[4]SZ[5]RE[B+R];B[aa];B[bb])(;GM[1]FF[4]SZ[5]RE[B+R];B[cc])",
        )
        .unwrap();

        assert!(import_sgf(sgf.to_str().unwrap(), csv.to_str().unwrap()).is_err());
        let stats =
            import_sgf_with_options(sgf.to_str().unwrap(), csv.to_str().unwrap(), true).unwrap();
        let games = load_games(csv.to_str().unwrap()).unwrap();
        std::fs::remove_file(sgf).unwrap();
        std::fs::remove_file(csv).unwrap();

        assert_eq!(stats.imported, 1);
        assert_eq!(stats.invalid, 1);
        assert_eq!(stats.invalid_examples.len(), 1);
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].moves, vec![12]);
    }

    #[test]
    fn go_dataset_roundtrips_setup_and_legacy_rows() {
        let path = std::env::temp_dir().join(format!("neurofour-go-{}.csv", std::process::id()));
        let games = [GoGame {
            size: 5,
            moves: vec![12, 25],
            winner: 0,
            result: Some("0".into()),
            komi: Some("6.5".into()),
            first_turn: -1,
            black_setup: vec![0, 1],
            white_setup: vec![23],
        }];
        save_games(path.to_str().unwrap(), &games).unwrap();
        let loaded = load_games(path.to_str().unwrap()).unwrap();
        std::fs::write(&path, "5,1,12 25\n").unwrap();
        let legacy = load_games(path.to_str().unwrap()).unwrap();
        std::fs::remove_file(path).unwrap();

        assert_eq!(loaded[0].first_turn, -1);
        assert_eq!(loaded[0].black_setup, vec![0, 1]);
        assert_eq!(loaded[0].white_setup, vec![23]);
        assert_eq!(loaded[0].winner, 0);
        assert_eq!(loaded[0].result.as_deref(), Some("0"));
        assert_eq!(loaded[0].komi.as_deref(), Some("6.5"));
        assert_eq!(legacy[0].first_turn, 1);
        assert!(legacy[0].black_setup.is_empty());
    }

    #[test]
    fn komi_feature_is_normalized_and_relative_to_side_to_move() {
        let mut board = GoBoard::new(5).unwrap();
        assert_eq!(
            board.features_with_komi(6.5).last().copied(),
            Some(-6.5 / 25.0)
        );
        board.play(board.pass_action()).unwrap();
        assert_eq!(
            board.features_with_komi(6.5).last().copied(),
            Some(6.5 / 25.0)
        );
    }

    #[test]
    fn game_validation_rejects_bad_winner_result_and_moves() {
        let game = GoGame {
            size: 5,
            moves: vec![12],
            winner: 1,
            result: Some("W+R".into()),
            komi: Some("6.5".into()),
            first_turn: 1,
            black_setup: Vec::new(),
            white_setup: Vec::new(),
        };
        assert_eq!(
            validate_game(&game),
            Err("Go winner does not match recorded result".into())
        );

        let mut invalid_move = game;
        invalid_move.result = None;
        invalid_move.moves = vec![25, 25, 12];
        assert!(validate_game(&invalid_move).is_err());
    }

    #[test]
    fn game_split_is_seeded_and_keeps_games_disjoint() {
        let games: Vec<_> = (0..5)
            .map(|action| GoGame {
                size: 5,
                moves: vec![action],
                winner: 1,
                result: None,
                komi: None,
                first_turn: 1,
                black_setup: Vec::new(),
                white_setup: Vec::new(),
            })
            .collect();
        let (training, validation) = split_games(&games, 5, 0.2, 17).unwrap();
        let (repeat_training, repeat_validation) = split_games(&games, 5, 0.2, 17).unwrap();

        assert_eq!(training.len(), 4);
        assert_eq!(validation.len(), 1);
        assert_eq!(
            training.iter().map(|game| &game.moves).collect::<Vec<_>>(),
            repeat_training
                .iter()
                .map(|game| &game.moves)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            validation
                .iter()
                .map(|game| &game.moves)
                .collect::<Vec<_>>(),
            repeat_validation
                .iter()
                .map(|game| &game.moves)
                .collect::<Vec<_>>()
        );
        assert!(training
            .iter()
            .all(|train| validation.iter().all(|valid| train.moves != valid.moves)));
        assert!(split_games(&games[..1], 5, 0.2, 17).is_err());
        assert!(split_games(&games, 5, 1.0, 17).is_err());
    }

    #[test]
    fn model_evaluates_held_out_moves_without_training() {
        let model = GoModel::new(5);
        let game = GoGame {
            size: 5,
            moves: vec![0, 1],
            winner: 1,
            result: None,
            komi: Some("0.5".into()),
            first_turn: 1,
            black_setup: Vec::new(),
            white_setup: Vec::new(),
        };

        assert_eq!(model.evaluate_loss(&[game]).unwrap(), (1.0, 2));
        assert!(model.evaluate_loss(&[]).is_err());
    }

    #[test]
    fn policy_top1_reports_tie_averaged_random_baseline() {
        let model = GoModel::new(5);
        let game = GoGame {
            size: 5,
            moves: vec![12, 13],
            winner: 1,
            result: None,
            komi: Some("0.5".into()),
            first_turn: 1,
            black_setup: Vec::new(),
            white_setup: Vec::new(),
        };
        let (accuracy, random_baseline, moves) = model.evaluate_policy_top1(&[game]).unwrap();

        assert_eq!(accuracy, random_baseline);
        assert_eq!(moves, 2);
        assert!(model.evaluate_policy_top1(&[]).is_err());
    }

    #[test]
    fn validated_training_keeps_the_best_model() {
        let game = GoGame {
            size: 5,
            moves: vec![12],
            winner: 1,
            result: None,
            komi: Some("0.5".into()),
            first_turn: 1,
            black_setup: Vec::new(),
            white_setup: Vec::new(),
        };
        let mut model = GoModel::new(5);
        let (best_epoch, _, best_loss) = model
            .train_with_validation(&[game.clone()], &[game.clone()], 2, 42)
            .unwrap();

        assert!(best_epoch > 0);
        assert!(best_loss < 1.0);
        assert_eq!(model.evaluate_loss(&[game]).unwrap().0, best_loss);
    }

    #[test]
    fn model_checkpoint_roundtrips_and_loads_legacy_format() {
        let path =
            std::env::temp_dir().join(format!("neurofour-go-model-{}.ngo", std::process::id()));
        let mut model = GoModel::new(5);
        model.value[0] = 0.25;
        let last_feature = model.value.len() - 1;
        model.value[last_feature] = -0.5;
        model.policy[0] = 0.75;
        model.save(path.to_str().unwrap()).unwrap();
        let loaded = GoModel::load(path.to_str().unwrap()).unwrap();
        assert_eq!(loaded.value, model.value);
        assert_eq!(loaded.policy, model.policy);

        let old_features = 5 * 5 * 2 + 1;
        let old_policy_len = (5 * 5 + 1) * old_features;
        let mut legacy = format!("NGO1 5\n");
        legacy.push_str(&format!("{:.1}\n", 0.0).repeat(old_features + old_policy_len));
        std::fs::write(&path, legacy).unwrap();
        let legacy_model = GoModel::load(path.to_str().unwrap()).unwrap();
        std::fs::remove_file(path).unwrap();

        assert_eq!(legacy_model.value.len(), old_features + 1);
        assert_eq!(legacy_model.policy.len(), (5 * 5 + 1) * (old_features + 1));
        assert!(legacy_model.value.iter().all(|weight| *weight == 0.0));
    }

    #[test]
    fn sgf_export_and_import_preserve_record_metadata() {
        let base = std::env::temp_dir().join(format!("neurofour-sgf-{}", std::process::id()));
        let data = base.with_extension("csv");
        let sgf = base.with_extension("sgf");
        let imported = base.with_extension("roundtrip.csv");
        let games = [GoGame {
            size: 5,
            moves: vec![12, 25, 18],
            winner: -1,
            result: Some("W+R".into()),
            komi: Some("0.50".into()),
            first_turn: -1,
            black_setup: vec![0, 5],
            white_setup: vec![24],
        }];
        save_games(data.to_str().unwrap(), &games).unwrap();
        export_sgf(data.to_str().unwrap(), sgf.to_str().unwrap()).unwrap();
        import_sgf(sgf.to_str().unwrap(), imported.to_str().unwrap()).unwrap();
        let roundtrip = load_games(imported.to_str().unwrap()).unwrap();
        for path in [&data, &sgf, &imported] {
            std::fs::remove_file(path).unwrap();
        }

        assert_eq!(roundtrip[0].size, games[0].size);
        assert_eq!(roundtrip[0].moves, games[0].moves);
        assert_eq!(roundtrip[0].winner, games[0].winner);
        assert_eq!(roundtrip[0].result, games[0].result);
        assert_eq!(roundtrip[0].first_turn, games[0].first_turn);
        assert_eq!(roundtrip[0].black_setup, games[0].black_setup);
        assert_eq!(roundtrip[0].white_setup, games[0].white_setup);
    }
}
