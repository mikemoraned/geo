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
