#!/bin/bash

# Enhanced Demo Script for Dynamic Proxy
# This script runs the enhanced demo with Redis and TLS support

echo "Dynamic Proxy - Enhanced Demo"
echo "============================"
echo
echo "This demo shows advanced proxy functionality with Redis backend and TLS."
echo "Prerequisites:"
echo "  - Redis server running on 127.0.0.1:6379"
echo "  - TLS certificates in certs/ directory"
echo
echo "The proxy will listen on:"
echo "  - TCP: 127.0.0.1:8080"
echo "  - UDP: 127.0.0.1:8081"
echo "  - Metrics: 127.0.0.1:9090"
echo

# Change to project root directory
cd "$(dirname "$0")/../.."

# Check Redis connection
echo "Checking Redis connection..."
if ! redis-cli ping > /dev/null 2>&1; then
    echo "❌ Redis server is not available at 127.0.0.1:6379"
    echo "Please start Redis server first:"
    echo "  sudo systemctl start redis"
    echo "  # or"
    echo "  redis-server"
    exit 1
fi
echo "✓ Redis server is available"

# Setup Redis backends if needed
echo "Setting up Redis backends..."
if [ -f "scripts/setup_redis_backends.sh" ]; then
    bash scripts/setup_redis_backends.sh
else
    echo "⚠ Redis setup script not found, continuing without backend setup"
fi

# 检查是否已构建
if [ ! -f "target/debug/dynamic-proxy" ]; then
    echo "Building project..."
    cargo build
    if [ $? -ne 0 ]; then
        echo "Build failed!"
        exit 1
    fi
fi

echo
echo "Starting enhanced proxy demo..."
echo "Press Ctrl+C to stop"
echo

# 启动代理服务器
cargo run --example enhanced_demo
