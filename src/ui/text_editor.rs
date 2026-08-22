//! A minimal multiline text editor state for in-TUI testcase editing.
//!
//! Backed by a `Vec<String>` of lines with a `(row, col)` cursor. Supports char
//! insertion, Backspace, Delete, Enter (newline), arrow navigation, Home/End
//! (Ctrl+A/Ctrl+E) and Ctrl+U (delete to start of line).

#[derive(Debug, Clone)]
pub struct TextEditor {
    pub lines: Vec<String>,
    pub cursor: (usize, usize), // (row, col)
}

impl Default for TextEditor {
    fn default() -> Self {
        Self::new(String::new())
    }
}

impl TextEditor {
    pub fn new(text: String) -> Self {
        let lines = if text.is_empty() {
            vec![String::new()]
        } else {
            text.split('\n').map(|l| l.to_string()).collect()
        };
        let row = lines.len() - 1;
        let col = lines.last().map(|l| l.chars().count()).unwrap_or(0);
        Self {
            lines,
            cursor: (row, col),
        }
    }

    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    pub fn clamp_cursor(&mut self) {
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        let nrows = self.lines.len();
        if self.cursor.0 >= nrows {
            self.cursor.0 = nrows - 1;
        }
        let ncols = self.lines[self.cursor.0].chars().count();
        if self.cursor.1 > ncols {
            self.cursor.1 = ncols;
        }
    }

    pub fn insert_char(&mut self, c: char) {
        let (row, col) = self.cursor;
        let line = &mut self.lines[row];
        let mut new_line = String::new();
        for (i, ch) in line.chars().enumerate() {
            if i == col {
                new_line.push(c);
            }
            new_line.push(ch);
        }
        if line.chars().count() == col {
            new_line.push(c);
        }
        self.lines[row] = new_line;
        self.cursor.1 += 1;
    }

    pub fn insert_newline(&mut self) {
        let (row, col) = self.cursor;
        let line = self.lines[row].clone();
        let left: String = line.chars().take(col).collect();
        let right: String = line.chars().skip(col).collect();
        self.lines[row] = left;
        self.lines.insert(row + 1, right);
        self.cursor = (row + 1, 0);
    }

    pub fn backspace(&mut self) {
        let (row, col) = self.cursor;
        if col == 0 {
            if row > 0 {
                let removed = self.lines.remove(row);
                let prev_len = self.lines[row - 1].chars().count();
                self.lines[row - 1].push_str(&removed);
                self.cursor = (row - 1, prev_len);
            }
        } else {
            let line = &mut self.lines[row];
            let mut new_line = String::new();
            let mut idx = 0;
            for ch in line.chars() {
                if idx != col - 1 {
                    new_line.push(ch);
                }
                idx += 1;
            }
            *line = new_line;
            self.cursor.1 -= 1;
        }
    }

    pub fn delete(&mut self) {
        let (row, col) = self.cursor;
        let line_len = self.lines[row].chars().count();
        if col < line_len {
            let line = &mut self.lines[row];
            let mut new_line = String::new();
            let mut idx = 0;
            for ch in line.chars() {
                if idx != col {
                    new_line.push(ch);
                }
                idx += 1;
            }
            *line = new_line;
        } else if row + 1 < self.lines.len() {
            let removed = self.lines.remove(row + 1);
            self.lines[row].push_str(&removed);
        }
    }

    pub fn move_left(&mut self) {
        let (row, col) = self.cursor;
        if col > 0 {
            self.cursor.1 -= 1;
        } else if row > 0 {
            self.cursor = (row - 1, self.lines[row - 1].chars().count());
        }
    }

    pub fn move_right(&mut self) {
        let (row, col) = self.cursor;
        if col < self.lines[row].chars().count() {
            self.cursor.1 += 1;
        } else if row + 1 < self.lines.len() {
            self.cursor = (row + 1, 0);
        }
    }

    pub fn move_up(&mut self) {
        if self.cursor.0 > 0 {
            self.cursor.0 -= 1;
            self.clamp_cursor();
        }
    }

    pub fn move_down(&mut self) {
        if self.cursor.0 + 1 < self.lines.len() {
            self.cursor.0 += 1;
            self.clamp_cursor();
        }
    }

    pub fn move_start(&mut self) {
        self.cursor.1 = 0;
    }

    pub fn move_end(&mut self) {
        let (row, _) = self.cursor;
        self.cursor.1 = self.lines[row].chars().count();
    }

    /// Row offset to scroll so the cursor is visible given a visible height.
    pub fn scroll_for_cursor(&self, visible_height: usize, scroll: usize) -> usize {
        let (row, _) = self.cursor;
        if row < scroll {
            row
        } else if row >= scroll + visible_height {
            row.saturating_sub(visible_height - 1)
        } else {
            scroll
        }
    }
}
