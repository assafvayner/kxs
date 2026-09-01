//! Kubernetes resource quantity parsing (`250m`, `1`, `128Mi`, `512M`, `1e3`…).

/// Splits a quantity into its numeric part and unit suffix. The longest
/// f64-parseable prefix wins, so `1e3` parses as scientific notation while
/// `1E` keeps `E` (exa) as the suffix, matching the apiserver.
fn split_number(q: &str) -> Option<(f64, &str)> {
    let mut end = None;
    for (i, _) in q.char_indices().skip(1) {
        if q[..i].parse::<f64>().is_ok() {
            end = Some(i);
        }
    }
    if q.parse::<f64>().is_ok() {
        end = Some(q.len());
    }
    let end = end?;
    let n: f64 = q[..end].parse().ok()?;
    n.is_finite().then_some((n, &q[end..]))
}

fn multiplier(suffix: &str) -> Option<f64> {
    Some(match suffix {
        "" => 1.0,
        "n" => 1e-9,
        "u" => 1e-6,
        "m" => 1e-3,
        "k" => 1e3,
        "M" => 1e6,
        "G" => 1e9,
        "T" => 1e12,
        "P" => 1e15,
        "E" => 1e18,
        "Ki" => 1024.0,
        "Mi" => 1024f64.powi(2),
        "Gi" => 1024f64.powi(3),
        "Ti" => 1024f64.powi(4),
        "Pi" => 1024f64.powi(5),
        "Ei" => 1024f64.powi(6),
        _ => return None,
    })
}

/// Parses a quantity into its value in base units (cores for CPU, bytes for
/// memory). `None` when the input is empty or not a quantity.
pub fn parse_quantity(q: &str) -> Option<f64> {
    let q = q.trim();
    if q.is_empty() {
        return None;
    }
    let (n, suffix) = split_number(q)?;
    Some(n * multiplier(suffix)?)
}

/// Parses a CPU quantity into millicores.
pub fn cpu_millis(q: &str) -> Option<i64> {
    parse_quantity(q).map(|cores| (cores * 1000.0).round() as i64)
}

/// Parses a memory quantity into mebibytes.
pub fn mem_mib(q: &str) -> Option<i64> {
    parse_quantity(q).map(|bytes| (bytes / 1024f64.powi(2)).round() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cpu_suffixes() {
        assert_eq!(cpu_millis("250m"), Some(250));
        assert_eq!(cpu_millis("1"), Some(1000));
        assert_eq!(cpu_millis("1.5"), Some(1500));
        assert_eq!(cpu_millis("2500000000n"), Some(2500));
        assert_eq!(cpu_millis("1500u"), Some(2));
        assert_eq!(cpu_millis(" 100m "), Some(100));
    }

    #[test]
    fn parses_binary_memory_suffixes() {
        assert_eq!(mem_mib("128Mi"), Some(128));
        assert_eq!(mem_mib("1Gi"), Some(1024));
        assert_eq!(mem_mib("1024Ki"), Some(1));
        assert_eq!(mem_mib("1Ti"), Some(1024 * 1024));
        assert_eq!(mem_mib("1Pi"), Some(1024 * 1024 * 1024));
        assert_eq!(mem_mib("1Ei"), Some(1024i64.pow(4)));
    }

    #[test]
    fn parses_decimal_memory_suffixes() {
        assert_eq!(mem_mib("512M"), Some(488)); // 512e6 bytes
        assert_eq!(mem_mib("1G"), Some(954));
        assert_eq!(mem_mib("1048576k"), Some(1000));
        assert_eq!(mem_mib("134217728"), Some(128)); // plain bytes
    }

    #[test]
    fn parses_exponent_notation_and_exa_suffix() {
        assert_eq!(parse_quantity("1e3"), Some(1000.0));
        assert_eq!(parse_quantity("1.5e-3"), Some(0.0015));
        assert_eq!(parse_quantity("1E"), Some(1e18));
        assert_eq!(parse_quantity("2Ei"), Some(2.0 * 1024f64.powi(6)));
    }

    #[test]
    fn rejects_non_quantities() {
        assert_eq!(parse_quantity(""), None);
        assert_eq!(parse_quantity("   "), None);
        assert_eq!(parse_quantity("garbage"), None);
        assert_eq!(parse_quantity("100Xi"), None);
        assert_eq!(parse_quantity("m"), None);
        assert_eq!(parse_quantity("inf"), None);
        assert_eq!(parse_quantity("NaN"), None);
    }

    #[test]
    fn parses_zero_and_negatives() {
        assert_eq!(mem_mib("0"), Some(0));
        assert_eq!(cpu_millis("0"), Some(0));
        assert_eq!(cpu_millis("-100m"), Some(-100));
    }
}
