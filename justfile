lint:
    cargo fmt --all -- --check
    cargo clippy --workspace --tests --examples --benches --bins -q -- -D warnings
    RUSTDOCFLAGS='-D warnings' cargo doc --workspace -q --no-deps --document-private-items

unit-tests:
    cargo test --release --all-features --lib

all-tests:
    cargo test --release --all-features

check-pr: lint all-tests

bench:
    cargo bench --all-features

run-peers:
    #!/usr/bin/env bash
    mkdir -p logs
    cargo build --workspace --release
    RUST_LOG="debug" ./target/release/bitservice-peer --party-id 0 --bind-addr 0.0.0.0:10000 --tcp-mpc-net-bind-addr 0.0.0.0:11000 --next-peer ws://localhost:10001/api/v1/ws --tcp-mpc-net-next-peer 127.0.0.1:11000 > logs/peer0.log 2>&1 &
    pid0=$!
    echo "started peer0 with PID $pid0"
    RUST_LOG="debug" ./target/release/bitservice-peer --party-id 1 --bind-addr 0.0.0.0:10001 --tcp-mpc-net-bind-addr 0.0.0.0:11001 --next-peer ws://localhost:10002/api/v1/ws --tcp-mpc-net-next-peer 127.0.0.1:11001 > logs/peer1.log 2>&1 &
    pid1=$!
    echo "started peer1 with PID $pid1"
    RUST_LOG="debug" ./target/release/bitservice-peer --party-id 2 --bind-addr 0.0.0.0:10002 --tcp-mpc-net-bind-addr 0.0.0.0:11002 --next-peer ws://localhost:10000/api/v1/ws --tcp-mpc-net-next-peer 127.0.0.1:11002 > logs/peer2.log 2>&1 &
    pid2=$!
    echo "started peer2 with PID $pid2"
    trap "kill $pid0 $pid1 $pid2" SIGINT SIGTERM
    wait $pid0 $pid1 $pid2

run-server:
    #!/usr/bin/env bash
    mkdir -p logs
    cargo build --workspace
    RUST_LOG="debug" ./target/debug/bitservice-server --rp-bitservice-peers-config rp_bitservice_peers_config.toml > logs/server.log 2>&1 &
    pid=$!
    echo "started server with PID $pid"
    trap "kill $pid" SIGINT SIGTERM
    wait $pid

