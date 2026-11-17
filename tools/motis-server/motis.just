TARGET := "macos-arm64"
# see releases page: https://github.com/motis-project/motis/releases for list
VERSION := "v2.11.4"

# Install host tools the recipes depend on. `brew install` is idempotent.
prerequisites:
    brew install yq
