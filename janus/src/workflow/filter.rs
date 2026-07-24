//! PTY output stream filter (ADR-018). Pure functions that clean raw tmux
//! pane output before it hits `truncate_16k` + `metamach_step_meta.stdout_tail`.
//!
//! Three stages, applied in order:
//! 1. `strip_ansi` — remove ANSI escape sequences (CSI / OSC / color codes).
//! 2. `collapse_progress_bars` — collapse repeated progress-indicator lines
//!    into a single summary.
//! 3. `deduplicate_lines` — collapse consecutive identical lines into a count.

/// Clean raw PTY output for storage in `stdout_tail`. Called by `run_steps`
/// after `capture_pane` and before `truncate_16k`.
pub fn clean_pty_output(raw: &str) -> String {
    let mut out = strip_ansi(raw);
    out = collapse_progress_bars(&out);
    out = deduplicate_lines(&out);
    out
}

// ── ANSI escape sequence stripping ──────────────────────────────────────

/// Remove ANSI CSI/OSC escape sequences from `s`. Handles:
/// - CSI: `ESC [` ... `A`-`Z` / `a`-`z` (colors, cursor movement, etc.)
/// - OSC: `ESC ]` ... `BEL` or `ESC \` (title changes, etc.)
/// - Simple: `ESC` followed by a single byte (RIS, etc.)
fn strip_ansi(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b && i + 1 < bytes.len() {
            match bytes[i + 1] {
                b'[' => {
                    // CSI: ESC [ ... <terminator>
                    i += 2; // skip ESC [
                    while i < bytes.len() && !is_csi_terminator(bytes[i]) {
                        i += 1;
                    }
                    if i < bytes.len() {
                        i += 1; // skip the terminator
                    }
                }
                b']' => {
                    // OSC: ESC ] ... BEL or ESC \
                    i += 2;
                    while i < bytes.len() && bytes[i] != 0x07 && bytes[i] != 0x1b {
                        i += 1;
                    }
                    if i < bytes.len()
                        && bytes[i] == 0x1b
                        && i + 1 < bytes.len()
                        && bytes[i + 1] == b'\\'
                    {
                        i += 2; // skip ESC \
                    } else if i < bytes.len() {
                        i += 1; // skip BEL
                    }
                }
                _ => {
                    // ESC followed by a single non-CSI/OSC byte (e.g. RIS = ESC c)
                    i += 2;
                }
            }
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    // Strip trailing ANSI reset left behind after all escapes are removed.
    let s = String::from_utf8_lossy(&out).into_owned();
    s.trim().to_string()
}

/// CSI sequences end with a byte in these ranges.
fn is_csi_terminator(b: u8) -> bool {
    (0x40..=0x7E).contains(&b) // @..~ covers all CSI final bytes
}

// ── Progress bar collapse ───────────────────────────────────────────────

/// Detect consecutive lines that look like progress indicators (e.g.
/// `[=====>  ] 45%`, `Writing... 67%`, `  45%`) and collapse them into one
/// line showing the first and last percentage seen.
fn collapse_progress_bars(s: &str) -> String {
    let lines: Vec<&str> = s.lines().collect();
    if lines.len() < 2 {
        return s.to_string();
    }
    let mut out = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        let pct = extract_progress_pct(lines[i]);
        if pct.is_none() {
            out.push(lines[i].to_string());
            i += 1;
            continue;
        }
        let first = pct.unwrap();
        let mut last = first;
        let mut j = i + 1;
        while j < lines.len() {
            if let Some(p) = extract_progress_pct(lines[j]) {
                last = p;
                j += 1;
            } else {
                break;
            }
        }
        if j > i + 1 {
            out.push(format!(
                "[MetaMach: progress {first}% → {last}% ({n} lines collapsed)]",
                n = j - i
            ));
        } else {
            out.push(lines[i].to_string());
        }
        i = j;
    }
    out.join("\n")
}

/// Extract a percentage from a progress-indicator line. Returns `Some(pct)`
/// if the line contains a percentage pattern, `None` otherwise.
fn extract_progress_pct(line: &str) -> Option<u32> {
    let trimmed = line.trim();
    // "  45%" or "45%" at end of line
    if let Some(pct_str) = trimmed.strip_suffix('%')
        && let Ok(p) = pct_str.trim().parse::<u32>()
        && p <= 100
    {
        return Some(p);
    }
    // "[=====>  ] 45%" — percentage somewhere in the line
    for word in trimmed.split_whitespace() {
        if let Some(pct_str) = word.strip_suffix('%')
            && let Ok(p) = pct_str.parse::<u32>()
            && p <= 100
        {
            return Some(p);
        }
    }
    None
}

// ── Duplicate line dedup ────────────────────────────────────────────────

/// Collapse consecutive identical lines into a count annotation.
/// "ACK\nACK\nACK\nDATA\n" → "ACK (×3)\nDATA\n"
fn deduplicate_lines(s: &str) -> String {
    let lines: Vec<&str> = s.lines().collect();
    let mut out = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        let mut count = 1usize;
        while i + count < lines.len() && lines[i + count] == lines[i] {
            count += 1;
        }
        if count > 1 {
            out.push(format!("{} (×{count})", lines[i]));
        } else {
            out.push(lines[i].to_string());
        }
        i += count;
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_ansi_removes_color_codes() {
        let input = "\x1b[32mHello\x1b[0m World";
        assert_eq!(strip_ansi(input), "Hello World");
    }

    #[test]
    fn strip_ansi_removes_cursor_movement() {
        let input = "\x1b[2J\x1b[1;1HReady\n";
        assert_eq!(strip_ansi(input), "Ready");
    }

    #[test]
    fn strip_ansi_preserves_plain_text() {
        let input = "cargo build --release\n   Compiling janus v0.4.2\n    Finished";
        assert_eq!(strip_ansi(input), input);
    }

    #[test]
    fn collapse_progress_bars_single_line() {
        let input = "Writing... 45%";
        let output = collapse_progress_bars(input);
        assert_eq!(output, input); // single line, no collapse
    }

    #[test]
    fn collapse_progress_bars_multiple_lines() {
        let input = "  0%\n 10%\n 20%\n 30%\nDone.";
        let output = collapse_progress_bars(input);
        assert!(output.contains("[MetaMach: progress 0% → 30%"));
        assert!(output.contains("Done."));
    }

    #[test]
    fn collapse_progress_bars_bracket_style() {
        let input = "[====>           ]  25%\n[=======>        ]  50%\n[==========>     ]  75%\n[===============] 100%";
        let output = collapse_progress_bars(input);
        assert!(output.contains("[MetaMach: progress 25% → 100%"));
    }

    #[test]
    fn collapse_progress_bars_non_progress_unchanged() {
        let input = "Compiling...\nerror: expected `;`\n  --> src/main.rs:10:5";
        assert_eq!(collapse_progress_bars(input), input);
    }

    #[test]
    fn deduplicate_lines_collapses_repeats() {
        let input = "ACK\nACK\nACK\nDATA\nACK";
        let output = deduplicate_lines(input);
        assert_eq!(output, "ACK (×3)\nDATA\nACK");
    }

    #[test]
    fn deduplicate_lines_no_repeats() {
        let input = "line1\nline2\nline3";
        assert_eq!(deduplicate_lines(input), input);
    }

    #[test]
    fn clean_pty_output_end_to_end() {
        let raw = "\x1b[32mCompiling\x1b[0m\n  0%\n 10%\n 20%\nDone.\nOK\nOK\nOK";
        let clean = clean_pty_output(raw);
        assert!(!clean.contains('\x1b')); // no ANSI
        assert!(clean.contains("[MetaMach: progress 0% → 20%"));
        assert!(clean.contains("OK (×3)"));
    }
}
