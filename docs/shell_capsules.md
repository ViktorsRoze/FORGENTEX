# Shell Capsule Hardening (v2.7)

Bash and PowerShell are the highest-risk capsule class: a shell capsule can
run arbitrary host commands. v2.7 admits them only behind strict review —
an explicit command allowlist, no wildcards, and reviewed host-path,
environment, and network exposure. Shell capsules are always at least
`review` and are never auto-approved.

## Command allowlist

A shell capsule must declare which commands it may run, as `exec.allow`
sandbox rules:

```fgx
foreign backup bash version "5.2" env "linux-x86_64" effects [proc.spawn, fs.read, fs.write]:
    sandbox "proc.spawn"
    sandbox "exec.allow tar"
    sandbox "exec.allow gzip"
    sandbox "fs.read /data/project"
    sandbox "fs.write /backups"
    code:
        tar -cf - /data/project | gzip > /backups/project.tar.gz
```

The allowlist refines the `proc.*` effect grant from the sandbox model
(v2.4): the effect says "this capsule spawns processes"; the allowlist says
"only these commands". `exec.allow` rules sit in the sandbox alongside the
filesystem, environment, and network grants.

## Validation (blocking at `fgx check`)

- `shell_capsule_no_allowlist` — a shell capsule must declare at least one
  `exec.allow <cmd>` rule.
- `shell_command_wildcard` — `exec.allow *` / `all` / `any` is forbidden;
  commands must be listed explicitly.

(Shell capsules also remain subject to v2.0–v2.4 rules: a `proc.spawn`
effect needs a `proc.*` sandbox grant, `fs.*` effects need `fs.*` grants,
etc.)

## Review report

```text
fgx shell-review <file> [--json] [--strict]
```

`forgentex.shell_review.v1` lists each shell capsule with its allowed
commands, host-path grants, environment reads, network grants, effects, and
a `review`/`reject` decision. `--strict` exits non-zero on `reject`. Notes
flag extra-risk conditions: network access from a shell capsule, and broad
host-path grants (`fs.* /`). Shell capsules always carry a "human approval
required" note.

## Example

`examples/capsule/backup_bash.fgx` — a backup capsule allowlisting only
`tar` and `gzip`, with host paths restricted to `/data/project` (read) and
`/backups` (write). It reviews as `review`. A capsule with no `exec.allow`
rule or a wildcard one is rejected at `fgx check`.

## Acceptance gates (v2.7)

- Bash/PowerShell/sh capsules classify as `shell`.
- A shell capsule with no command allowlist reports
  `shell_capsule_no_allowlist` and reviews as `reject`.
- A wildcard allowlist reports `shell_command_wildcard`.
- `fgx shell-review` lists allowed commands, host paths, env reads, and
  network grants, and `--strict` exits non-zero on rejection.
