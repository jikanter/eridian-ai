//! `--install-deps`: install the external companion tools aichat's workflows
//! lean on — `uv`, `showboat`, `pi` — skipping any already on PATH.
//!
//! The decision (install vs. skip) is a pure function over a presence probe so
//! it is testable without touching the real PATH or running an installer. The
//! caller supplies the probe and executes the resulting plan.

/// A companion tool: its display name, the binary to probe for on PATH, and the
/// shell command that installs it.
#[derive(Debug, Clone, PartialEq)]
pub struct DepSpec {
    pub name: &'static str,
    pub probe: &'static str,
    pub install_cmd: &'static str,
}

/// The companion tools, in install order. `uv` precedes `showboat` because the
/// canonical showboat install (`uv tool install showboat`) needs uv present.
pub fn default_deps() -> Vec<DepSpec> {
    vec![
        DepSpec {
            name: "uv",
            probe: "uv",
            install_cmd: "curl -LsSf https://astral.sh/uv/install.sh | sh",
        },
        DepSpec {
            name: "showboat",
            probe: "showboat",
            install_cmd: "uv tool install showboat",
        },
        DepSpec {
            name: "pi",
            probe: "pi",
            install_cmd: "npm install -g @earendil-works/pi-coding-agent",
        },
    ]
}

#[derive(Debug, Clone, PartialEq)]
pub enum DepAction {
    Install,
    SkipPresent,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DepPlanItem {
    pub name: String,
    pub install_cmd: String,
    pub action: DepAction,
}

/// Decide, per dep, whether to install or skip (already present). Pure over the
/// `is_present` probe; order is preserved from `deps`.
pub fn plan_install(deps: &[DepSpec], is_present: impl Fn(&str) -> bool) -> Vec<DepPlanItem> {
    deps.iter()
        .map(|d| DepPlanItem {
            name: d.name.to_string(),
            install_cmd: d.install_cmd.to_string(),
            action: if is_present(d.probe) {
                DepAction::SkipPresent
            } else {
                DepAction::Install
            },
        })
        .collect()
}

/// Render a plan as a human-readable preview (used for `--install-deps --dry-run`).
pub fn render_plan(plan: &[DepPlanItem]) -> String {
    let mut out = String::from("--- Install Deps ---\n");
    for item in plan {
        match item.action {
            DepAction::SkipPresent => {
                out.push_str(&format!("  {:<10} present, skip\n", item.name))
            }
            DepAction::Install => {
                out.push_str(&format!("  {:<10} install: {}\n", item.name, item.install_cmd))
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_deps_install_uv_before_showboat() {
        let deps = default_deps();
        let names: Vec<&str> = deps.iter().map(|d| d.name).collect();
        let uv = names.iter().position(|n| *n == "uv").unwrap();
        let sb = names.iter().position(|n| *n == "showboat").unwrap();
        assert!(uv < sb, "uv must be installed before showboat: {names:?}");
        assert!(names.contains(&"pi"));
    }

    #[test]
    fn present_deps_are_skipped_absent_are_installed() {
        let deps = default_deps();
        // Pretend only uv is already installed.
        let plan = plan_install(&deps, |bin| bin == "uv");

        let by_name = |n: &str| plan.iter().find(|p| p.name == n).unwrap();
        assert_eq!(by_name("uv").action, DepAction::SkipPresent);
        assert_eq!(by_name("showboat").action, DepAction::Install);
        assert_eq!(by_name("pi").action, DepAction::Install);
    }

    #[test]
    fn plan_preserves_dep_order() {
        let deps = default_deps();
        let plan = plan_install(&deps, |_| false);
        let plan_names: Vec<&str> = plan.iter().map(|p| p.name.as_str()).collect();
        let dep_names: Vec<&str> = deps.iter().map(|d| d.name).collect();
        assert_eq!(plan_names, dep_names);
    }

    #[test]
    fn plan_item_carries_the_install_command() {
        let deps = default_deps();
        let plan = plan_install(&deps, |_| false);
        let showboat = plan.iter().find(|p| p.name == "showboat").unwrap();
        assert_eq!(showboat.install_cmd, "uv tool install showboat");
    }

    #[test]
    fn render_plan_shows_command_for_install_and_skip_for_present() {
        let deps = default_deps();
        let plan = plan_install(&deps, |bin| bin == "uv");
        let out = render_plan(&plan);

        assert!(out.contains("uv"));
        assert!(out.contains("skip"), "present dep should show skip: {out}");
        assert!(
            out.contains("uv tool install showboat"),
            "absent dep should show its install command: {out}"
        );
    }
}
