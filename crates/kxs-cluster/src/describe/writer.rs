/// Tab-aligned text builder mirroring Go's text/tabwriter as kubectl uses it
/// (padding 2): each line is a list of cells; consecutive lines with two or
/// more cells form a block, and within a block every cell except a line's
/// last is padded to the widest cell in its column plus two spaces. Lines
/// with a single cell are emitted verbatim and end the block.
#[derive(Debug, Default)]
pub struct Writer {
    lines: Vec<Vec<String>>,
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
        self.lines
            .push(vec![format!("{}{key}:", indent(level)), value.to_string()]);
    }

    /// `Key:` with no inline value; children follow at `level + 1`.
    pub fn section(&mut self, level: usize, key: &str) {
        self.lines.push(vec![format!("{}{key}:", indent(level))]);
    }

    /// Continuation of the previous value: an empty first cell so the value
    /// aligns under the one above (second label, second toleration...).
    pub fn cont(&mut self, level: usize, value: &str) {
        self.lines.push(vec![indent(level), value.to_string()]);
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
        self.lines.push(v);
    }

    /// A single-cell line, emitted verbatim.
    pub fn text(&mut self, level: usize, text: &str) {
        self.lines.push(vec![format!("{}{text}", indent(level))]);
    }

    pub fn finish(self) -> String {
        let mut out = String::new();
        let lines = self.lines;
        let mut i = 0;
        while i < lines.len() {
            if lines[i].len() < 2 {
                out.push_str(lines[i][0].trim_end());
                out.push('\n');
                i += 1;
                continue;
            }
            let mut j = i;
            while j < lines.len() && lines[j].len() >= 2 {
                j += 1;
            }
            let block = &lines[i..j];
            let ncols = block.iter().map(Vec::len).max().unwrap_or(0);
            let mut widths = vec![0usize; ncols];
            for line in block {
                for (c, cell) in line.iter().enumerate().take(line.len() - 1) {
                    widths[c] = widths[c].max(cell.chars().count() + PAD);
                }
            }
            for line in block {
                let mut s = String::new();
                for (c, cell) in line.iter().enumerate() {
                    s.push_str(cell);
                    if c + 1 < line.len() {
                        s.push_str(&" ".repeat(widths[c] - cell.chars().count()));
                    }
                }
                out.push_str(s.trim_end());
                out.push('\n');
            }
            i = j;
        }
        out
    }
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
}
