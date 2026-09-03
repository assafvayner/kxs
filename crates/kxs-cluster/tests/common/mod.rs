//! Helpers shared by the integration tests. `kxs_cluster`'s own `#[cfg(test)]`
//! copy in `describe::util::test_support` is not visible from here.

/// Collapses alignment padding — trailing whitespace trimmed, leading
/// whitespace kept, internal runs of 2+ spaces reduced to two — so goldens do
/// not have to encode column widths.
pub fn normalize(value: &str) -> String {
    let mut output = String::new();
    for line in value.lines() {
        let line = line.trim_end();
        let leading = line.len() - line.trim_start().len();
        output.push_str(&line[..leading]);
        let mut spaces = 0;
        for character in line[leading..].chars() {
            if character == ' ' {
                spaces += 1;
                continue;
            }
            if spaces > 0 {
                output.push_str(if spaces > 1 { "  " } else { " " });
                spaces = 0;
            }
            output.push(character);
        }
        output.push('\n');
    }
    output
}
