#!/bin/sh
# Install the pinned Ratchet source revision used by ratchet.config.json.
set -eu
cargo install --git https://github.com/all-the-rest/ratchet-cli \
    --rev 817a68c --locked ratchet
test "$(ratchet --version)" = "ratchet 0.5.0"
