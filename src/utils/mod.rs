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
