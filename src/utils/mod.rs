mod abort_signal;
mod clipboard;
mod command;
mod crypto;
pub mod exit_code;
mod html_to_md;
mod input;
pub mod ledger;
mod loader;
mod path;
mod render_prompt;
mod request;
mod spinner;
pub mod trace;
pub mod trace_spec;
mod variables;

pub use self::abort_signal::*;
pub use self::clipboard::set_text;
pub use self::command::*;
pub use self::crypto::*;
pub use self::exit_code::{classify_error, is_retryable_stage_error, AichatError, ExitCode};
pub use self::html_to_md::*;
pub use self::input::*;
pub use self::loader::*;
pub use self::path::*;
pub use self::render_prompt::render_prompt;
pub use self::request::*;
pub use self::spinner::*;
pub use self::variables::*;

use anyhow::{Context, Result};
use fancy_regex::Regex;
use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};
use is_terminal::IsTerminal;
use std::borrow::Cow;
use std::sync::LazyLock;
use std::{env, path::PathBuf, process};
use unicode_segmentation::UnicodeSegmentation;

pub static CODE_BLOCK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?ms)```\w*(.*)```").unwrap());
pub static THINK_TAG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)^\s*<think>.*?</think>(\s*|$)").unwrap());
pub static IS_STDOUT_TERMINAL: LazyLock<bool> = LazyLock::new(|| std::io::stdout().is_terminal());

/// When to colorize output. Backs the `--color` flag (Phase 54B).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum ColorWhen {
    /// Colorize when stdout is a terminal and NO_COLOR is unset (default).
    #[default]
    Auto,
    /// Always colorize, even when piped.
    Always,
    /// Never colorize.
    Never,
}

// Global `--color` override. 0 = Auto (unset), 1 = Always, 2 = Never.
static COLOR_OVERRIDE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Record the `--color` choice. Call once, early in startup, before any
/// colorized output or config color resolution.
pub fn set_color_when(when: ColorWhen) {
    let v = match when {
        ColorWhen::Auto => 0,
        ColorWhen::Always => 1,
        ColorWhen::Never => 2,
    };
    COLOR_OVERRIDE.store(v, std::sync::atomic::Ordering::Relaxed);
}

fn color_override() -> ColorWhen {
    match COLOR_OVERRIDE.load(std::sync::atomic::Ordering::Relaxed) {
        1 => ColorWhen::Always,
        2 => ColorWhen::Never,
        _ => ColorWhen::Auto,
    }
}

/// Pure color-suppression decision. `env_no_color` is the parsed NO_COLOR env
/// value (None when unset/unparseable), `is_tty` whether stdout is a terminal.
fn decide_no_color(when: ColorWhen, env_no_color: Option<bool>, is_tty: bool) -> bool {
    match when {
        ColorWhen::Never => true,
        ColorWhen::Always => false,
        ColorWhen::Auto => env_no_color.unwrap_or(false) || !is_tty,
    }
}

/// Whether color output is suppressed, honoring `--color`, then NO_COLOR, then
/// TTY detection. Replaces the former `NO_COLOR` static so `--color` can win.
pub fn no_color() -> bool {
    let env_no_color = env::var("NO_COLOR").ok().and_then(|v| parse_bool(&v));
    decide_no_color(color_override(), env_no_color, *IS_STDOUT_TERMINAL)
}

// Global `--quiet` flag (Phase 54B). Suppresses the spinner and the cost line.
static QUIET: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Record the `--quiet` choice. Call once, early in startup.
pub fn set_quiet(quiet: bool) {
    QUIET.store(quiet, std::sync::atomic::Ordering::Relaxed);
}

/// Whether quiet mode is on.
pub fn is_quiet() -> bool {
    QUIET.load(std::sync::atomic::Ordering::Relaxed)
}

/// Pure predicate for whether the spinner should be suppressed: off when not a
/// TTY, in quiet mode, or with an empty message.
pub fn spinner_suppressed(is_tty: bool, quiet: bool, empty_message: bool) -> bool {
    !is_tty || quiet || empty_message
}

/// Whether the cost summary should print: requested via `--cost` and not quiet.
pub fn should_show_cost(cost_flag: bool, quiet: bool) -> bool {
    cost_flag && !quiet
}

/// Levenshtein edit distance between two strings (insert/delete/substitute).
fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr = vec![0usize; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        curr[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b.len()]
}

/// Nearest candidate to `input` by edit distance, if within `max_distance`.
/// Powers "did you mean ...?" suggestions for unknown role/model/agent names.
/// On ties, the first candidate at the minimum distance wins.
pub fn nearest_match(input: &str, candidates: &[String], max_distance: usize) -> Option<String> {
    candidates
        .iter()
        .map(|c| (c, levenshtein(input, c)))
        .filter(|(_, d)| *d <= max_distance)
        .min_by_key(|(_, d)| *d)
        .map(|(c, _)| c.clone())
}

/// Append a "did you mean `X`?" hint to `base` when a near candidate exists.
pub fn did_you_mean(base: &str, input: &str, candidates: &[String]) -> String {
    // Allow a slightly looser bound for longer inputs.
    let max_distance = (input.chars().count() / 3).max(2);
    match nearest_match(input, candidates, max_distance) {
        Some(s) => format!("{base}. Did you mean `{s}`?"),
        None => base.to_string(),
    }
}

/// Whether stdin is an interactive terminal. Paired with `IS_STDOUT_TERMINAL`;
/// used by the non-interactive input policy (Phase 54C).
pub static IS_STDIN_TERMINAL: LazyLock<bool> = LazyLock::new(|| std::io::stdin().is_terminal());

/// What to do when a destructive action needs confirmation.
#[derive(Debug, PartialEq, Eq)]
pub enum ConfirmAction {
    /// Proceed without asking (`--yes`).
    Proceed,
    /// Ask the user interactively.
    Prompt,
    /// Refuse: no `--yes` and no way to ask (non-TTY or `--no-input`).
    Refuse,
}

/// Whether an interactive prompt is possible: stdin is a TTY and `--no-input`
/// was not given.
pub fn can_prompt(stdin_is_tty: bool, no_input: bool) -> bool {
    stdin_is_tty && !no_input
}

/// Resolve how to handle a destructive confirmation: `--yes` proceeds; else
/// prompt when interactive; else refuse rather than hang.
pub fn resolve_confirm(yes: bool, can_prompt: bool) -> ConfirmAction {
    if yes {
        ConfirmAction::Proceed
    } else if can_prompt {
        ConfirmAction::Prompt
    } else {
        ConfirmAction::Refuse
    }
}

pub fn now() -> String {
    chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false)
}

pub fn now_timestamp() -> i64 {
    chrono::Local::now().timestamp()
}

pub fn get_env_name(key: &str) -> String {
    format!("{}_{key}", env!("CARGO_CRATE_NAME"),).to_ascii_uppercase()
}

pub fn normalize_env_name(value: &str) -> String {
    value.replace('-', "_").to_ascii_uppercase()
}

pub fn parse_bool(value: &str) -> Option<bool> {
    match value {
        "1" | "true" => Some(true),
        "0" | "false" => Some(false),
        _ => None,
    }
}

pub fn estimate_token_length(text: &str) -> usize {
    let words: Vec<&str> = text.unicode_words().collect();
    let mut output: f32 = 0.0;
    for word in words {
        if word.is_ascii() {
            output += 1.3;
        } else {
            let count = word.chars().count();
            if count == 1 {
                output += 1.0
            } else {
                output += (count as f32) * 0.5;
            }
        }
    }
    output.ceil() as usize
}

pub fn strip_think_tag(text: &str) -> Cow<'_, str> {
    THINK_TAG_RE.replace_all(text, "")
}

pub fn extract_code_block(text: &str) -> &str {
    CODE_BLOCK_RE
        .captures(text)
        .ok()
        .and_then(|v| v?.get(1).map(|v| v.as_str().trim()))
        .unwrap_or(text)
}

pub fn convert_option_string(value: &str) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

pub fn fuzzy_filter<T, F>(values: Vec<T>, get: F, pattern: &str) -> Vec<T>
where
    F: Fn(&T) -> &str,
{
    let matcher = SkimMatcherV2::default();
    let mut list: Vec<(T, i64)> = values
        .into_iter()
        .filter_map(|v| {
            let score = matcher.fuzzy_match(get(&v), pattern)?;
            Some((v, score))
        })
        .collect();
    list.sort_unstable_by(|a, b| b.1.cmp(&a.1));
    list.into_iter().map(|(v, _)| v).collect()
}

pub fn pretty_error(err: &anyhow::Error) -> String {
    let mut output = vec![];
    output.push(format!("Error: {err}"));
    let causes: Vec<_> = err.chain().skip(1).collect();
    let causes_len = causes.len();
    if causes_len > 0 {
        output.push("\nCaused by:".to_string());
        if causes_len == 1 {
            output.push(format!("    {}", indent_text(causes[0], 4).trim()));
        } else {
            for (i, cause) in causes.into_iter().enumerate() {
                output.push(format!("{i:5}: {}", indent_text(cause, 7).trim()));
            }
        }
    }
    output.join("\n")
}

pub fn indent_text<T: ToString>(s: T, size: usize) -> String {
    let indent_str = " ".repeat(size);
    s.to_string()
        .split('\n')
        .map(|line| format!("{indent_str}{line}"))
        .collect::<Vec<String>>()
        .join("\n")
}

pub fn error_text(input: &str) -> String {
    color_text(input, nu_ansi_term::Color::Red)
}

pub fn warning_text(input: &str) -> String {
    color_text(input, nu_ansi_term::Color::Yellow)
}

pub fn color_text(input: &str, color: nu_ansi_term::Color) -> String {
    if no_color() {
        return input.to_string();
    }
    nu_ansi_term::Style::new()
        .fg(color)
        .paint(input)
        .to_string()
}

pub fn dimmed_text(input: &str) -> String {
    if no_color() {
        return input.to_string();
    }
    nu_ansi_term::Style::new().dimmed().paint(input).to_string()
}

pub fn multiline_text(input: &str) -> String {
    input
        .split('\n')
        .enumerate()
        .map(|(i, v)| {
            if i == 0 {
                v.to_string()
            } else {
                format!(".. {v}")
            }
        })
        .collect::<Vec<String>>()
        .join("\n")
}

pub fn temp_file(prefix: &str, suffix: &str) -> PathBuf {
    env::temp_dir().join(format!(
        "{}-{}{prefix}{}{suffix}",
        env!("CARGO_CRATE_NAME").to_lowercase(),
        process::id(),
        uuid::Uuid::new_v4()
    ))
}

pub fn is_url(path: &str) -> bool {
    path.starts_with("http://") || path.starts_with("https://")
}

pub fn set_proxy(
    mut builder: reqwest::ClientBuilder,
    proxy: &str,
) -> Result<reqwest::ClientBuilder> {
    builder = builder.no_proxy();
    if !proxy.is_empty() && proxy != "-" {
        builder = builder
            .proxy(reqwest::Proxy::all(proxy).with_context(|| format!("Invalid proxy `{proxy}`"))?);
    };
    Ok(builder)
}

pub fn decode_bin<T: serde::de::DeserializeOwned>(data: &[u8]) -> Result<T> {
    let (v, _) = bincode::serde::decode_from_slice(data, bincode::config::legacy())?;
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_override_forces_regardless_of_env_and_tty() {
        // `--color=never` (ColorWhen::Never) disables color even on a TTY.
        assert!(decide_no_color(ColorWhen::Never, Some(false), true));
        assert!(decide_no_color(ColorWhen::Never, None, true));
        // `--color=always` enables color even when piped (non-TTY) and with NO_COLOR set.
        assert!(!decide_no_color(ColorWhen::Always, Some(true), false));
        assert!(!decide_no_color(ColorWhen::Always, None, false));
    }

    #[test]
    fn spinner_suppressed_when_quiet_or_non_tty_or_empty() {
        // Shown only on a TTY, not quiet, with a non-empty message.
        assert!(!spinner_suppressed(true, false, false));
        // Quiet suppresses even on a TTY with a message.
        assert!(spinner_suppressed(true, true, false));
        // Non-TTY suppresses.
        assert!(spinner_suppressed(false, false, false));
        // Empty message suppresses.
        assert!(spinner_suppressed(true, false, true));
    }

    #[test]
    fn nearest_match_suggests_close_typo() {
        let cands = ["summarize".to_string(), "translate".to_string()];
        assert_eq!(
            nearest_match("summarise", &cands, 2).as_deref(),
            Some("summarize")
        );
    }

    #[test]
    fn nearest_match_exact_is_distance_zero() {
        let cands = ["translate".to_string(), "summarize".to_string()];
        assert_eq!(
            nearest_match("translate", &cands, 2).as_deref(),
            Some("translate")
        );
    }

    #[test]
    fn nearest_match_none_when_too_far_or_empty() {
        let cands = ["summarize".to_string(), "translate".to_string()];
        assert_eq!(nearest_match("zzzzzzzz", &cands, 2), None);
        let empty: [String; 0] = [];
        assert_eq!(nearest_match("summarize", &empty, 2), None);
    }

    #[test]
    fn can_prompt_requires_tty_and_no_no_input() {
        assert!(can_prompt(true, false)); // interactive TTY, input allowed
        assert!(!can_prompt(true, true)); // --no-input forces off even on a TTY
        assert!(!can_prompt(false, false)); // piped stdin => cannot prompt
        assert!(!can_prompt(false, true));
    }

    #[test]
    fn resolve_confirm_yes_proceeds_else_prompt_else_refuse() {
        // --yes always proceeds, even non-interactive.
        assert_eq!(resolve_confirm(true, false), ConfirmAction::Proceed);
        assert_eq!(resolve_confirm(true, true), ConfirmAction::Proceed);
        // No --yes but interactive => prompt.
        assert_eq!(resolve_confirm(false, true), ConfirmAction::Prompt);
        // No --yes and non-interactive => refuse (don't hang).
        assert_eq!(resolve_confirm(false, false), ConfirmAction::Refuse);
    }

    #[test]
    fn cost_shows_only_when_requested_and_not_quiet() {
        assert!(should_show_cost(true, false));
        assert!(!should_show_cost(true, true)); // quiet overrides --cost
        assert!(!should_show_cost(false, false));
        assert!(!should_show_cost(false, true));
    }

    #[test]
    fn quiet_override_roundtrips() {
        set_quiet(true);
        assert!(is_quiet());
        set_quiet(false);
        assert!(!is_quiet());
    }

    #[test]
    fn color_auto_follows_env_and_tty() {
        // auto + interactive TTY + no NO_COLOR => color on.
        assert!(!decide_no_color(ColorWhen::Auto, None, true));
        // auto + piped (non-TTY) => color off.
        assert!(decide_no_color(ColorWhen::Auto, None, false));
        // auto + TTY but NO_COLOR truthy => color off.
        assert!(decide_no_color(ColorWhen::Auto, Some(true), true));
        // auto + TTY + NO_COLOR explicitly false => color on.
        assert!(!decide_no_color(ColorWhen::Auto, Some(false), true));
    }

    #[test]
    #[cfg(not(target_os = "windows"))]
    fn test_safe_join_path() {
        assert_eq!(
            safe_join_path("/home/user/dir1", "files/file1"),
            Some(PathBuf::from("/home/user/dir1/files/file1"))
        );
        assert!(safe_join_path("/home/user/dir1", "/files/file1").is_none());
        assert!(safe_join_path("/home/user/dir1", "../file1").is_none());
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn test_safe_join_path() {
        assert_eq!(
            safe_join_path("C:\\Users\\user\\dir1", "files/file1"),
            Some(PathBuf::from("C:\\Users\\user\\dir1\\files\\file1"))
        );
        assert!(safe_join_path("C:\\Users\\user\\dir1", "/files/file1").is_none());
        assert!(safe_join_path("C:\\Users\\user\\dir1", "../file1").is_none());
    }
}
