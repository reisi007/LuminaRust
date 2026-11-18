#!/bin/sh
# Install the pinned Ratchet source revision used by ratchet.config.json.
set -eu
cargo install --git https://github.com/all-the-rest/ratchet-cli \
    --tag v0.8.0 --locked ratchet
test "$(ratchet --version)" = "ratchet 0.8.0"
