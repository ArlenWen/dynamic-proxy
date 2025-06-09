#!/bin/bash

# Basic Demo Script for Dynamic Proxy
# This script runs the basic usage example with minimal configuration

echo "Dynamic Proxy - Basic Demo"
echo "========================="
echo
echo "This demo shows basic proxy functionality without TLS or Redis."
echo "The proxy will listen on:"
echo "  - TCP: 127.0.0.1:18080"
echo "  - UDP: 127.0.0.1:18081"
echo "  - Metrics: 127.0.0.1:19090"
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

echo "Starting basic proxy demo..."
echo "Press Ctrl+C to stop"
echo

# 启动代理服务器
cargo run --example basic_usage
