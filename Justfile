# Launch Claude Code under the safehouse sandbox, with read-only access to the shared ESP-IDF install.
claude:
    safehouse --add-dirs-ro "$HOME/.espressif" claude --permission-mode auto

# Build the repo-wide tools an app relies on; a fresh checkout needs it once.
prerequisites:
    cd tools/prose-gate && just build
