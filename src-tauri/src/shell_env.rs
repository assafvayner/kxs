//! Recover the user's real PATH at startup.
//!
//! A GUI-launched app on macOS (Finder/dock) inherits launchd's minimal PATH
//! — `/usr/bin:/bin:/usr/sbin:/sbin` — which omits Homebrew (`/opt/homebrew/bin`,
//! `/usr/local/bin`) and other user install dirs. Exec-based kubeconfig auth
//! then can't find its helper: `aws eks get-token` for EKS, `gke-gcloud-auth-plugin`,
//! `kubelogin`, `aws-iam-authenticator`, etc. kube-rs runs `Command::new("aws")`,
//! the PATH lookup misses, and the failure surfaces as
//! `unable to run auth exec: No such file or directory (os error 2)`.
//!
//! We probe the login shell once and merge its PATH into the process
//! environment so every exec plugin resolves the same binaries the user's
//! terminal would.

/// Merge the process PATH so exec-auth helpers resolve. No-op on Windows,
/// whose GUI processes inherit the full user PATH already.
pub fn augment_path() {
    #[cfg(not(target_os = "windows"))]
    {
        let current = std::env::var("PATH").unwrap_or_default();
        let merged = merge_paths(login_shell_path().as_deref(), &current, FALLBACK_DIRS);
        // Safe: called at the top of `main`, before any threads are spawned.
        std::env::set_var("PATH", merged);
    }
}

#[cfg(not(target_os = "windows"))]
const FALLBACK_DIRS: &[&str] = &["/opt/homebrew/bin", "/opt/homebrew/sbin", "/usr/local/bin"];

/// Ask the login shell for its PATH. Interactive + login (`-ilc`) so both
/// `.zprofile`/`.zprofile`-style login files (Homebrew's `shellenv`) and
/// `.zshrc`-style interactive files (nvm, cargo, ...) are sourced. The value
/// is bracketed by sentinels so rc-file chatter on stdout can't corrupt it.
#[cfg(not(target_os = "windows"))]
fn login_shell_path() -> Option<String> {
    const BEGIN: &str = "__KXS_PATH_BEGIN__";
    const END: &str = "__KXS_PATH_END__";
    let shell = std::env::var("SHELL").ok()?;
    let script = format!("printf '%s%s%s' '{BEGIN}' \"$PATH\" '{END}'");
    let output = std::process::Command::new(&shell)
        .args(["-ilc", &script])
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let start = stdout.find(BEGIN)? + BEGIN.len();
    let rest = &stdout[start..];
    let end = rest.find(END)?;
    let path = &rest[..end];
    (!path.is_empty()).then(|| path.to_string())
}

/// Build a PATH from the shell-derived entries first (the user's real order),
/// then whatever the process already had, then fallbacks — deduping while
/// preserving first-seen order.
#[cfg_attr(target_os = "windows", allow(dead_code))]
fn merge_paths(shell: Option<&str>, current: &str, fallbacks: &[&str]) -> String {
    let mut ordered: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut push = |dir: &str| {
        if !dir.is_empty() && seen.insert(dir.to_string()) {
            ordered.push(dir.to_string());
        }
    };
    for dir in shell
        .into_iter()
        .chain(std::iter::once(current))
        .flat_map(|s| s.split(':'))
    {
        push(dir);
    }
    for dir in fallbacks {
        push(dir);
    }
    ordered.join(":")
}

#[cfg(test)]
mod tests {
    use super::merge_paths;

    #[test]
    fn dedups_and_preserves_first_seen_order() {
        let out = merge_paths(
            Some("/opt/homebrew/bin:/usr/bin"),
            "/usr/bin:/bin",
            &["/usr/local/bin", "/opt/homebrew/bin"],
        );
        assert_eq!(out, "/opt/homebrew/bin:/usr/bin:/bin:/usr/local/bin");
    }

    #[test]
    fn falls_back_when_shell_probe_absent() {
        let out = merge_paths(None, "/usr/bin:/bin", &["/opt/homebrew/bin"]);
        assert_eq!(out, "/usr/bin:/bin:/opt/homebrew/bin");
    }

    #[test]
    fn skips_empty_segments() {
        let out = merge_paths(Some(""), "/usr/bin::/bin:", &[""]);
        assert_eq!(out, "/usr/bin:/bin");
    }
}
