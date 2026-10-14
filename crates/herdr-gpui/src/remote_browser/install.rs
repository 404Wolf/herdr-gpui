//! Puts the `herdr-gpui` shim, and the agent skill naming it, on each saved
//! SSH host a window connects to, once per run and only when the user chose
//! to install the skill. Like the local skill, a file is written only where
//! it is missing or carries this app's marker: a file of the same name the
//! user wrote is left alone, and nothing else on the host changes.
use crate::{
    HerdrWindow,
    agent_skill::{AgentSkill, Choice},
};
use gpui::{Context, Global};
use herdr_client::{ScriptHost, ScriptLimits, run_script};
use std::{collections::HashSet, sync::atomic::AtomicBool, time::Duration};

pub(super) const SHIM: &str = include_str!("herdr-gpui.sh");
/// Where agents on the host run the shim from; their shell expands `~`.
pub(super) const SHIM_COMMAND: &str = "~/.local/state/herdr/gpui/bin/herdr-gpui";

/// Writes standard input to the shim's path unless the user has a file of
/// their own there.
pub(super) const SHIM_SCRIPT: &str = r#"set -eu
dir="$HOME/.local/state/herdr/gpui/bin"
file="$dir/herdr-gpui"
if [ -e "$file" ] && ! grep -q herdr-gpui-managed-shim "$file"; then exit 0; fi
mkdir -p "$dir"
tmp="$file.new.$$"
trap 'rm -f "$tmp"' EXIT
cat >"$tmp"
chmod 755 "$tmp"
mv -f "$tmp" "$file""#;

/// Writes standard input as the skill in each agent directory the host has,
/// as the app does on this machine.
pub(super) const SKILL_SCRIPT: &str = r#"set -eu
text=$(cat)
for root in "$HOME/.claude" "$HOME/.agents"; do
    [ -d "$root" ] || continue
    dir="$root/skills/herdr-gpui-browser"
    file="$dir/SKILL.md"
    if [ -e "$file" ] && ! grep -q herdr-gpui-managed-skill "$file"; then continue; fi
    mkdir -p "$dir"
    printf '%s\n' "$text" >"$file.new.$$"
    mv -f "$file.new.$$" "$file"
done"#;

const LIMITS: ScriptLimits = ScriptLimits {
    output: 4096,
    idle: Duration::from_secs(30),
};

/// The hosts tried this run, by SSH target, whether or not it worked: a
/// host without a POSIX shell is not asked again on every tick.
#[derive(Default)]
struct Tried(HashSet<String>);

impl Global for Tried {}

/// Blocking: two short SSH sessions. Run off the UI thread.
fn install(target: &str) -> crate::Result<()> {
    let host = ScriptHost::Ssh(target);
    let never = AtomicBool::new(false);
    run_script(
        host,
        SHIM_SCRIPT,
        SHIM.as_bytes(),
        std::io::sink(),
        LIMITS,
        &never,
    )
    .map_err(crate::Error::RemoteAgentTools)?;
    let skill = crate::agent_skill::text_naming(SHIM_COMMAND);
    run_script(
        host,
        SKILL_SCRIPT,
        skill.as_bytes(),
        std::io::sink(),
        LIMITS,
        &never,
    )
    .map_err(crate::Error::RemoteAgentTools)?;
    Ok(())
}

impl HerdrWindow {
    /// Starts installing on each connected saved SSH host not tried yet.
    pub(super) fn install_remote_agent_tools(&mut self, cx: &mut Context<Self>) {
        if AgentSkill::choice(cx) != Some(Choice::Installed) {
            return;
        }
        for (index, endpoint) in self.endpoints.iter().enumerate() {
            let live = if index == self.selected_endpoint {
                &self.live
            } else {
                &endpoint.live
            };
            let Some((target, _)) = endpoint.saved_ssh() else {
                continue;
            };
            if !endpoint.enabled || live.snapshot.is_none() {
                continue;
            }
            if !cx.default_global::<Tried>().0.insert(target.to_owned()) {
                continue;
            }
            let target = target.to_owned();
            cx.background_executor()
                .spawn(async move {
                    match install(&target) {
                        Ok(()) => tracing::info!("Installed the agent tools on an SSH host"),
                        Err(error) => {
                            tracing::warn!(%error, "Cannot install the agent tools on an SSH host");
                        }
                    }
                })
                .detach();
        }
    }
}
