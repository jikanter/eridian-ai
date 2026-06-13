//! `--exec-pi` passthrough.
//!
//! When `--exec-pi` is the *first* CLI argument, aichat loads its own
//! customizations and then hands every remaining argument to `pi -p
//! <args...>`, propagating pi's exit status. The flag is only honored in the
//! first position so an ordinary run like `aichat chat --exec-pi` is left
//! untouched.

/// The flag that triggers the pi passthrough. Must be the first argument.
pub const EXEC_PI_FLAG: &str = "--exec-pi";

/// If `args` (process arguments *excluding* argv[0]) begins with `--exec-pi`,
/// return the remaining arguments to forward to `pi -p`. Returns `None` when
/// the flag is absent or not in the first position.
pub fn exec_pi_args(args: &[String]) -> Option<Vec<String>> {
    match args.split_first() {
        Some((first, rest)) if first == EXEC_PI_FLAG => Some(rest.to_vec()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn forwards_remaining_args_when_flag_is_first() {
        let args = argv(&["--exec-pi", "List all .ts files", "in", "src/"]);
        assert_eq!(
            exec_pi_args(&args),
            Some(argv(&["List all .ts files", "in", "src/"]))
        );
    }

    #[test]
    fn flag_alone_yields_empty_passthrough() {
        let args = argv(&["--exec-pi"]);
        assert_eq!(exec_pi_args(&args), Some(vec![]));
    }

    #[test]
    fn flag_not_in_first_position_is_ignored() {
        let args = argv(&["chat", "--exec-pi", "hello"]);
        assert_eq!(exec_pi_args(&args), None);
    }

    #[test]
    fn no_args_yields_none() {
        assert_eq!(exec_pi_args(&[]), None);
    }
}
