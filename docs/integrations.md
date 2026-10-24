# Integrations

Everything talks to the island through the `ngambang` command (a local,
current-user-only named pipe). If the island is not running, commands exit
with code 2 — or 0 with `--quiet`, which is what hooks use.

## Any script

```powershell
ngambang notify "deploy staging" --sticky            # running job, ring shows it
ngambang notify "deploy staging" --progress 0.4      # update progress (0..1 or 0..100)
ngambang done   "deploy staging" --ok                # ✓ tag with the elapsed time
ngambang done   "deploy staging" --fail --code 1     # ✕ tag with an "Open terminal" button
ngambang notify "Backup finished" --level ok         # one-off tag
```

Or wrap a command so it reports itself:

```powershell
ngambang run -- cargo build --release
ngambang run --id tests -- npm test
```

`run` keeps the command's exit code, so it is safe in scripts.

## Shell hooks (long commands)

Commands that run longer than `[commands] long_command_s` (30 s by default)
report when they end. Commands in `[commands] ignore` never do.

| Shell | Add to | Line |
|-------|--------|------|
| PowerShell 5/7 | `$PROFILE` | `ngambang hook powershell \| Out-String \| Invoke-Expression` |
| bash (Git Bash, WSL, MSYS2) | `~/.bashrc` | `eval "$(ngambang.exe hook bash)"` |
| zsh (WSL, MSYS2) | `~/.zshrc` | `eval "$(ngambang.exe hook zsh)"` |

In WSL the Windows `PATH` is appended by default, so `ngambang.exe` resolves
as long as the folder you unzipped Ngambang into is on your Windows `PATH`.

## Git hooks

`.git/hooks/post-merge`:

```sh
#!/bin/sh
ngambang.exe notify "Pulled $(git rev-parse --abbrev-ref HEAD)" --level ok --quiet
```

## Claude Code

In `.claude/settings.json`, tell the island when the agent stops or needs you:

```json
{
  "hooks": {
    "Stop": [{ "hooks": [{ "type": "command", "command": "ngambang notify \"Claude finished\" --level ok --quiet" }] }],
    "Notification": [{ "hooks": [{ "type": "command", "command": "ngambang notify \"Claude needs you\" --level warn --quiet" }] }]
  }
}
```

## Protocol

For other languages, connect to `\\.\pipe\ngambang-<your SID>` and write one
JSON object per line (`"v": 1`). The full message list is in
[SPEC.md §6.2](SPEC.md#62-pipe-protocol-v1).
