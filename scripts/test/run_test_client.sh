#!/bin/bash

# Test Client Script for Dynamic Proxy
# This script runs the test client to verify proxy functionality

echo "Dynamic Proxy - Test Client"
echo "=========================="
echo
echo "This script runs the test client to verify proxy functionality."
echo "Make sure a proxy demo is running before executing this test."
echo
echo "Expected proxy endpoints:"
echo "  - TCP: 127.0.0.1:18080 (basic demo)"
echo "  - UDP: 127.0.0.1:18081 (basic demo)"
echo "  - Metrics: 127.0.0.1:19090 (basic demo)"
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

echo "Running test client..."
echo

# 运行测试客户端
cargo run --example test_client
