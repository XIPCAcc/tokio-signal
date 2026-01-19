#!/bin/bash

# Unix signal IPC benchmark test script
trap '' SIGUSR1 SIGUSR2  # Ignore signals to prevent interruption

# Set default count or use provided value
COUNT=${1:-100}

# Simple usage check
if [[ "$COUNT" =~ ^- ]]; then
    echo "Usage: $0 [COUNT]" >&2
    exit 1
fi

echo "=== tokio-signal Benchmark ==="
echo "Count: $COUNT"
echo

# Compile and run
echo "1. Compiling..."
cargo build --release || exit 1

rm -f server_output.txt

# Launch server in background
echo "2. Running server..."
./target/release/tokio-signal -m server -c $COUNT -s 1 &
SERVER_PID=$!

# Small delay to ensure server is ready
sleep 1

# Run client in foreground
echo "3. Running client..."
./target/release/tokio-signal -m client -c $COUNT -s 1

# Wait for server to complete
echo "4. Waiting for server..."
wait $SERVER_PID