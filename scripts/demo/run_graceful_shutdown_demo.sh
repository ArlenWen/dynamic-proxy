#!/bin/bash

# Graceful Shutdown Demo Script for Dynamic Proxy
# This script runs the graceful shutdown demo

echo "Dynamic Proxy - Graceful Shutdown Demo"
echo "======================================"
echo
echo "This demo shows how the proxy handles graceful shutdown signals."
echo "You can test different shutdown signals:"
echo "  - Ctrl+C (SIGINT): Graceful shutdown"
echo "  - kill -TERM <pid>: Graceful shutdown"
echo "  - kill -QUIT <pid>: Force shutdown"
echo "  - kill -HUP <pid>: Configuration reload (graceful shutdown)"
echo
echo "The proxy will listen on:"
echo "  - TCP: 127.0.0.1:28080"
echo "  - UDP: 127.0.0.1:28081"
echo "  - Metrics: 127.0.0.1:29090"
echo

# Change to project root directory
cd "$(dirname "$0")/../.."

# 检查是否已构建
if [ ! -f "target/debug/dynamic-proxy" ]; then
    echo "Building project..."
    cargo build
    if [ $? -ne 0 ]; then
        echo "Build failed!"
        exit 1
    fi
fi

echo "Starting graceful shutdown demo..."
echo "Process ID will be displayed for testing signals"
echo "Press Ctrl+C to test graceful shutdown"
echo

# 启动代理服务器
cargo run --example graceful_shutdown_demo
