# Runs with cwd = apps/<app> so the slice skills resolve `docs/` to that app,
# grants read/write within apps/<app> (safehouse workdir), and read-only across
# the whole repo so Claude can discover the shared `.claude/` skills+rules and
# read `CLAUDE.md`.
#
# Launch Claude Code for an app under the safehouse sandbox (e.g. `just claude lookout`).
claude app:
    #!/usr/bin/env bash
    set -euo pipefail
    if [ ! -d "apps/{{app}}" ]; then
        echo "no such app: apps/{{app}}" >&2
        exit 1
    fi
    cd "apps/{{app}}"
    exec safehouse --add-dirs-ro "{{justfile_directory()}}" claude --permission-mode auto
