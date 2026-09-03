//! Copy helper: OSC 52 escape plus a local pbcopy/xclip/wl-copy fallback.

/// Copies text to the clipboard. Best-effort, never fails the caller.
pub fn copy(text: &str) {
    // local tools first so it works over SSH-without-OSC too
    let tools: [&[&str]; 3] = [
        &["pbcopy"],
        &["xclip", "-selection", "clipboard"],
        &["wl-copy"],
    ];
    for tool in tools {
        if let Ok(mut child) = std::process::Command::new(tool[0])
            .args(&tool[1..])
            .stdin(std::process::Stdio::piped())
            .spawn()
        {
            if let Some(stdin) = child.stdin.as_mut() {
                use std::io::Write;
                let _ = stdin.write_all(text.as_bytes());
            }
            let _ = child.wait();
            return;
        }
    }
    // OSC 52 for terminals that support it
    use base64::Engine;
    let enc = base64::engine::general_purpose::STANDARD.encode(text);
    print!("\x1b]52;c;{enc}\x07");
    let _ = std::io::Write::flush(&mut std::io::stdout());
}
