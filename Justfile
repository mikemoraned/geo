# Runs with cwd = apps/<app> so the slice skills resolve `docs/` to that app,
# grants read/write within apps/<app> (safehouse workdir), and read-only across
# the whole repo so Claude can discover the shared `.claude/` skills+rules and
# read `CLAUDE.md`.
#
# `~/.espressif` is read-only too: a device shell builds against one shared ESP-IDF install
# there rather than several gigabytes under each git worktree. Safehouse denies by default and
# grants nothing outside what is listed, so without this the build fails on a bare "operation
# not permitted" reading a path that plainly exists. Read-only is enough — only the first
# install writes there, and that one is run outside the sandbox.
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
    exec safehouse --add-dirs-ro "{{justfile_directory()}}:$HOME/.espressif" claude --permission-mode auto
