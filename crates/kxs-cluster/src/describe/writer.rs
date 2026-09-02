/// Tab-aligned text builder mirroring Go's text/tabwriter as kubectl uses it
/// (padding 2): each line is a list of cells; consecutive lines with two or
/// more cells form a block, and within a block every cell except a line's
/// last is padded to the widest cell in its column plus two spaces. Lines
/// with a single cell are emitted verbatim and end the block.
#[derive(Debug, Default)]
pub struct Writer {
    lines: Vec<Line>,
}

#[derive(Debug)]
struct Line {
    cells: Vec<String>,
    trim_end: bool,
}

const PAD: usize = 2;

fn indent(n: usize) -> String {
    "  ".repeat(n)
}

impl Writer {
    pub fn new() -> Self {
        Self::default()
    }

    /// `Key:` then `value` in the next cell.
    pub fn kv(&mut self, level: usize, key: &str, value: impl std::fmt::Display) {
        self.push(
            vec![format!("{}{key}:", indent(level)), value.to_string()],
            true,
        );
    }

    /// `Key:` with no inline value; children follow at `level + 1`.
    pub fn section(&mut self, level: usize, key: &str) {
        self.push(vec![format!("{}{key}:", indent(level))], true);
    }

    /// Continuation of the previous value: an empty first cell so the value
    /// aligns under the one above (second label, second toleration...).
    pub fn cont(&mut self, level: usize, value: &str) {
        self.push(vec![indent(level), value.to_string()], true);
    }

    /// Arbitrary cells (table header and rows).
    pub fn cells(&mut self, level: usize, cells: &[&str]) {
        if cells.is_empty() {
            self.text(level, "");
            return;
        }
        let mut v: Vec<String> = cells.iter().map(|c| c.to_string()).collect();
        if let Some(first) = v.first_mut() {
            *first = format!("{}{first}", indent(level));
        }
        self.push(v, true);
    }

    /// A single-cell line, emitted verbatim.
    pub fn text(&mut self, level: usize, text: &str) {
        self.push(vec![format!("{}{text}", indent(level))], true);
    }

    /// A single-cell line whose trailing whitespace is content, not padding.
    pub fn preserved_text(&mut self, level: usize, text: &str) {
        self.push(vec![format!("{}{text}", indent(level))], false);
    }

    fn push(&mut self, cells: Vec<String>, trim_end: bool) {
        self.lines.push(Line { cells, trim_end });
    }

    pub fn finish(self) -> String {
        let mut out = String::new();
        let mut lines = self.lines;
        for line in &mut lines {
            for cell in &mut line.cells {
                *cell = escape_terminal(cell);
            }
        }
        let mut i = 0;
        while i < lines.len() {
            if lines[i].cells.len() < 2 {
                let text = &lines[i].cells[0];
                out.push_str(if lines[i].trim_end {
                    text.trim_end()
                } else {
                    text
                });
                out.push('\n');
                i += 1;
                continue;
            }
            let mut j = i;
            while j < lines.len() && lines[j].cells.len() >= 2 {
                j += 1;
            }
            let block = &lines[i..j];
            let ncols = block.iter().map(|line| line.cells.len()).max().unwrap_or(0);
            let mut widths = vec![0usize; ncols];
            for line in block {
                for (c, cell) in line.cells.iter().enumerate().take(line.cells.len() - 1) {
                    widths[c] = widths[c].max(cell.chars().count() + PAD);
                }
            }
            for line in block {
                let mut s = String::new();
                for (c, cell) in line.cells.iter().enumerate() {
                    s.push_str(cell);
                    if c + 1 < line.cells.len() {
                        s.push_str(&" ".repeat(widths[c] - cell.chars().count()));
                    }
                }
                out.push_str(if line.trim_end { s.trim_end() } else { &s });
                out.push('\n');
            }
            i = j;
        }
        out
    }
}

/// Mirrors kubectl's `WriteEscaped`: terminal controls are made visible while
/// line feeds remain structural output.
fn escape_terminal(value: &str) -> String {
    value.replace('\u{1b}', "^[").replace('\r', "\\r")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aligns_values_in_a_block() {
        let mut w = Writer::new();
        w.kv(0, "Name", "web");
        w.kv(0, "Namespace", "default");
        w.kv(0, "IP", "10.0.0.1");
        assert_eq!(
            w.finish(),
            "Name:       web\nNamespace:  default\nIP:         10.0.0.1\n"
        );
    }

    #[test]
    fn section_breaks_alignment_and_indents_children() {
        let mut w = Writer::new();
        w.kv(0, "Name", "web");
        w.section(0, "Containers");
        w.section(1, "web");
        w.kv(2, "Image", "nginx");
        w.kv(2, "Restart Count", 0);
        assert_eq!(
            w.finish(),
            "Name:  web\nContainers:\n  web:\n    Image:          nginx\n    Restart Count:  0\n"
        );
    }

    #[test]
    fn continuation_lines_align_under_the_value() {
        let mut w = Writer::new();
        w.kv(0, "Labels", "app=web");
        w.cont(0, "tier=frontend");
        w.kv(0, "Annotations", "<none>");
        assert_eq!(
            w.finish(),
            "Labels:       app=web\n              tier=frontend\nAnnotations:  <none>\n"
        );
    }

    #[test]
    fn tables_align_every_column_but_the_last() {
        let mut w = Writer::new();
        w.section(0, "Events");
        w.cells(1, &["Type", "Reason", "Message"]);
        w.cells(1, &["----", "------", "-------"]);
        w.cells(1, &["Normal", "Scheduled", "Successfully assigned"]);
        assert_eq!(
            w.finish(),
            "Events:\n  Type    Reason     Message\n  ----    ------     -------\n  Normal  Scheduled  Successfully assigned\n"
        );
    }

    #[test]
    fn lines_with_fewer_cells_do_not_widen_missing_columns() {
        let mut w = Writer::new();
        w.cells(0, &["a", "bb", "c"]);
        w.cells(0, &["dddd", "e"]);
        assert_eq!(w.finish(), "a     bb  c\ndddd  e\n");
    }

    #[test]
    fn trailing_whitespace_is_trimmed_and_text_is_verbatim() {
        let mut w = Writer::new();
        w.text(0, "");
        w.text(0, "Data");
        w.text(0, "====");
        w.kv(0, "k", "");
        assert_eq!(w.finish(), "\nData\n====\nk:\n");
    }

    #[test]
    fn empty_cells_render_a_blank_line() {
        let mut w = Writer::new();
        w.cells(2, &[]);
        assert_eq!(w.finish(), "\n");
    }

    #[test]
    fn preserved_text_keeps_trailing_and_whitespace_only_lines() {
        let mut w = Writer::new();
        w.preserved_text(0, "first\u{1b}  \r\n   \nlast \r ");
        assert_eq!(w.finish(), "first^[  \\r\n   \nlast \\r \n");
    }

    #[test]
    fn terminal_controls_are_escaped_before_alignment() {
        let mut w = Writer::new();
        w.kv(0, "abc\u{1b}", "first\r");
        w.kv(0, "long", "second\u{1b}");
        assert_eq!(w.finish(), "abc^[:  first\\r\nlong:   second^[\n");
    }
}
