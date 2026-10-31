use std::fmt;

pub const ROWS: usize = 6;
pub const COLS: usize = 7;
pub const CELLS: usize = ROWS * COLS;
pub const BASE_FEATURES: usize = CELLS * 2 + 1;
pub const FEATURES: usize = BASE_FEATURES + COLS * 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Board {
    pub cells: [i8; CELLS],
    pub turn: i8,
    pub moves: u8,
}

impl Default for Board {
    fn default() -> Self {
        Self {
            cells: [0; CELLS],
            turn: 1,
            moves: 0,
        }
    }
}

impl Board {
    pub fn legal_moves(&self) -> Vec<usize> {
        (0..COLS)
            .filter(|&c| self.cells[(ROWS - 1) * COLS + c] == 0)
            .collect()
    }
    pub fn play(&mut self, col: usize) -> Result<(), String> {
        if col >= COLS {
            return Err("column out of range".into());
        }
        for row in 0..ROWS {
            let i = row * COLS + col;
            if self.cells[i] == 0 {
                self.cells[i] = self.turn;
                self.turn = -self.turn;
                self.moves += 1;
                return Ok(());
            }
        }
        Err("column full".into())
    }
    pub fn winner(&self) -> i8 {
        for r in 0..ROWS {
            for c in 0..COLS {
                let p = self.cells[r * COLS + c];
                if p == 0 {
                    continue;
                }
                for (dr, dc) in [(1, 0), (0, 1), (1, 1), (1, -1)] {
                    let mut ok = true;
                    for k in 1..4 {
                        let rr = r as isize + dr * k;
                        let cc = c as isize + dc * k;
                        if rr < 0
                            || rr >= ROWS as isize
                            || cc < 0
                            || cc >= COLS as isize
                            || self.cells[rr as usize * COLS + cc as usize] != p
                        {
                            ok = false;
                            break;
                        }
                    }
                    if ok {
                        return p;
                    }
                }
            }
        }
        0
    }
    pub fn terminal(&self) -> bool {
        self.winner() != 0 || self.moves as usize == CELLS
    }
    pub fn features(&self) -> [f32; FEATURES] {
        let mut x = [0.0; FEATURES];
        for i in 0..CELLS {
            if self.cells[i] == self.turn {
                x[i] = 1.0
            } else if self.cells[i] == -self.turn {
                x[CELLS + i] = 1.0
            }
        }
        // Keep the historical bias index stable for checkpoint compatibility.
        x[BASE_FEATURES - 1] = 1.0;
        for col in self.legal_moves() {
            let mut own = *self;
            own.play(col).unwrap();
            if own.winner() == self.turn {
                x[BASE_FEATURES + col] = 1.0;
            }
            let mut opponent = *self;
            opponent.turn = -self.turn;
            opponent.play(col).unwrap();
            if opponent.winner() == -self.turn {
                x[BASE_FEATURES + COLS + col] = 1.0;
            }
        }
        x
    }
    pub fn mirror(&self) -> Self {
        let mut b = *self;
        for r in 0..ROWS {
            for c in 0..COLS {
                b.cells[r * COLS + c] = self.cells[r * COLS + (COLS - 1 - c)];
            }
        }
        b
    }
}

impl fmt::Display for Board {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for r in (0..ROWS).rev() {
            for c in 0..COLS {
                write!(
                    f,
                    "{} ",
                    match self.cells[r * COLS + c] {
                        1 => 'X',
                        -1 => 'O',
                        _ => '.',
                    }
                )?;
            }
            writeln!(f)?;
        }
        writeln!(f, "0 1 2 3 4 5 6")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detects_winner() {
        let mut b = Board::default();
        for c in [0, 0, 1, 1, 2, 2, 3] {
            b.play(c).unwrap();
        }
        assert_eq!(b.winner(), 1);
    }
    #[test]
    fn mirror_roundtrip() {
        let b = Board::default();
        assert_eq!(b, b.mirror().mirror());
    }
}
