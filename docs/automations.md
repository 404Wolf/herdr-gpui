# Automations

Automations are short Luau scripts that run steps on a host on a schedule or
when someone starts them: pull a branch, run validation, start an agent in a
new worktree with a prompt. They are meant to be written by an agent from a
sentence ("every day at 3am, pull main, run validation, then start an agent
with this prompt") and reviewed by a person before they run.

**Status: phase 1.** `herdr-automate check` loads a script and prints its
plan, and `herdr-automate steps` lists the steps. Nothing runs scripts yet:
the daemon that schedules and runs them, approval, and the Automations
section in Herdr UI come next.

## A script

Scripts live in `.herdr/automations/NAME.luau` in a repository, or in
`~/.config/herdr-automate/automations/` for personal ones.

```lua
herdr.caps { agents_per_run = 2, tokens_per_day = 2_000_000, wall_time = "3h" }

herdr.at("03:00", { tz = "Europe/Paris" }, function(fire)
  pull("main")
  local v = sh("just ci", { check = false })
  if not v.ok then
    agent {
      harness = "claude",
      worktree = "fix-validation-" .. fire.date,
      prompt = "Nightly validation failed on main:\n" .. v.log_tail .. "\nFix it.",
    }
  end
  agent { harness = "claude", worktree = "nightly-" .. fire.date, prompt = "..." }
end)

herdr.manual({ name = "deploy-preview" }, function(fire)
  sh("./scripts/deploy-preview.sh")
  notify("Preview deployed")
end)
```

`herdr-automate check nightly.luau` prints:

```text
nightly
  caps: 2 agents per run · 2,000,000 tokens/day · 3h per run
  daily at 03:00 (Europe/Paris)
    1. pull main
    2. sh `just ci` (arbitrary command) · the script handles failure
       └ on failure, instead:
           - agent claude in worktree fix-validation-<date>, prompt 72 chars
           - agent claude in worktree nightly-<date>, prompt 3 chars
    3. agent claude in worktree nightly-<date>, prompt 3 chars
  deploy-preview: manual only
    1. sh `./scripts/deploy-preview.sh` (arbitrary command)
    2. notify "Preview deployed"

OK: 2 triggers. Every trigger can also be started by hand.
```

`--json` prints the same plan as JSON, or `{"ok": false, "error": ...}`.
The exit status is 0 for a valid script, 1 for an invalid or unreadable one,
and 2 for a usage error.

## Triggers

| Declaration | Fires |
| --- | --- |
| `herdr.at("03:00", { days = "mon-fri", tz = "Europe/Paris" }, fn)` | At a time of day; `days` is `daily` (default), `weekdays`, `weekends`, a range or a list |
| `herdr.every("6h", fn)` | Every interval, from 1m to 30d |
| `herdr.cron("*/15 9-17 * * mon-fri", { tz = ... }, fn)` | On a five-field cron expression |
| `herdr.manual(fn)` | Only when someone starts it |

Every trigger can also be started by hand. Each takes an optional
`{ name = "..." }`, unique within the script; a script has at most 16
triggers. Times are read in the host's zone unless `tz` names one.

The handler gets `fire`: `fire.reason` is `"schedule"`, `"late"` (the
schedule came round while the runner was down) or `"manual"`, with
`fire.date` (`YYYY-MM-DD`) and `fire.time` (`HH:MM`).

## Caps

`herdr.caps` is required, once, at the top level: `agents_per_run` (0 to
16), `tokens_per_day` (up to 1,000,000,000) and `wall_time` (1m to 1d). They
print in the plan, so they are approved with it.

## Steps

| Step | Does |
| --- | --- |
| `agent { harness, prompt, worktree?, base?, wait? }` | Start an agent in a new worktree and send it the prompt |
| `notify("text")` | Post a notification |
| `pull("main")` | Fast-forward a branch in the main checkout; fails if it is dirty or has diverged |
| `run("setup")`, `run("run")` | Run the setup or run script from `.herdr/worktree.toml` |
| `sh("command")` | Run a shell command; the plan marks it as an arbitrary command |
| `worktree { name, base? }` | Create a worktree |

A step that fails stops the run. With `{ check = false }` it returns
`{ ok, exit, duration, log_tail, run_id }` instead and the script decides.
Steps also take `{ timeout = "30m" }`. Unknown fields, control characters in
names, and over-long text are refused with the field named.

## What a script cannot do

Steps are a script's only effects. The sandbox has no `os`, `io`, `debug`,
`require`, `loadstring`, `print`, `pcall`, `math.random` or coroutines, and
the libraries are read-only. So the same script always makes the same calls,
and the plan, which runs each trigger with every step succeeding and then
once per step with that step failing, shows everything it can do. Steps are
refused at the top level, and triggers inside a handler.

A script's own computation is bounded: about 10 million interrupt checks per
load or run, 32 MiB of memory, 32 steps per run, and 256 KiB of source.
`pcall` is gone so a stopped step or a budget always ends the run.

Luau's `--!strict` comment is accepted, but `check` does not type-check yet:
the Luau type checker is not part of the embedded interpreter.
