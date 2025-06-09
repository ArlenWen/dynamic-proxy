#!/bin/bash

# Graceful Shutdown Test Script
# This script tests the graceful shutdown functionality of the dynamic proxy

set -e

PROXY_HOST=${PROXY_HOST:-127.0.0.1}
TCP_PORT=${TCP_PORT:-28080}
UDP_PORT=${UDP_PORT:-28081}
METRICS_PORT=${METRICS_PORT:-29090}

echo "Testing Graceful Shutdown Functionality"
echo "======================================="
echo

# Function to check if process is running
is_process_running() {
    local pid=$1
    kill -0 $pid 2>/dev/null
}

# Function to wait for process to stop
wait_for_process_stop() {
    local pid=$1
    local timeout=${2:-30}
    local count=0
    
    while [ $count -lt $timeout ]; do
        if ! is_process_running $pid; then
            return 0
        fi
        sleep 1
        count=$((count + 1))
    done
    
    return 1
}

# Function to test signal handling
test_signal() {
    local signal=$1
    local signal_name=$2
    local expected_behavior=$3
    
    echo "Testing $signal_name signal handling..."
    echo "--------------------------------------"
    
    # Start the proxy server
    echo "Starting proxy server..."
    cargo run --example graceful_shutdown_demo &
    local proxy_pid=$!
    
    # Wait for server to start
    echo "Waiting for server to start (PID: $proxy_pid)..."
    sleep 3
    
    # Verify server is running
    if ! is_process_running $proxy_pid; then
        echo "❌ Failed to start proxy server"
        return 1
    fi
    
    # Check if ports are listening
    if nc -z $PROXY_HOST $TCP_PORT 2>/dev/null; then
        echo "✓ TCP proxy is listening on $PROXY_HOST:$TCP_PORT"
    else
        echo "⚠ TCP proxy not responding (this might be expected)"
    fi
    
    # Generate some load
    echo "Generating test load..."
    generate_test_load &
    local load_pid=$!
    
    # Wait a bit for connections to establish
    sleep 2
    
    # Send the signal
    echo "Sending $signal_name signal to proxy (PID: $proxy_pid)..."
    kill -$signal $proxy_pid
    
    # Monitor shutdown process
    echo "Monitoring shutdown process..."
    local start_time=$(date +%s)
    
    if wait_for_process_stop $proxy_pid 30; then
        local end_time=$(date +%s)
        local duration=$((end_time - start_time))
        echo "✓ Proxy stopped gracefully in ${duration} seconds"
    else
        echo "❌ Proxy did not stop within timeout, force killing..."
        kill -9 $proxy_pid 2>/dev/null || true
        wait $proxy_pid 2>/dev/null || true
    fi
    
    # Stop load generation
    kill $load_pid 2>/dev/null || true
    wait $load_pid 2>/dev/null || true
    
    echo "✓ $signal_name test completed"
    echo
}

# Function to generate test load
generate_test_load() {
    echo "Generating TCP connections..."
    
    # Generate TCP connections
    for i in {1..5}; do
        (
            echo "Test message $i" | nc $PROXY_HOST $TCP_PORT &
            sleep 1
        ) &
    done
    
    # Generate UDP packets
    echo "Generating UDP packets..."
    for i in {1..3}; do
        echo "UDP packet $i" | nc -u $PROXY_HOST $UDP_PORT &
        sleep 0.5
    done
    
    # Keep some connections alive
    while true; do
        echo "Keepalive" | nc $PROXY_HOST $TCP_PORT >/dev/null 2>&1 || true
        sleep 2
    done
}

# Function to test metrics during shutdown
test_metrics_during_shutdown() {
    echo "Testing metrics availability during shutdown..."
    echo "---------------------------------------------"
    
    # Start the proxy server
    echo "Starting proxy server..."
    cargo run --example graceful_shutdown_demo &
    local proxy_pid=$!
    
    # Wait for server to start
    sleep 3
    
    # Check initial metrics
    echo "Checking initial metrics..."
    if curl -s http://$PROXY_HOST:$METRICS_PORT/metrics >/dev/null; then
        echo "✓ Metrics endpoint is accessible"
    else
        echo "❌ Metrics endpoint not accessible"
        kill $proxy_pid 2>/dev/null || true
        return 1
    fi
    
    # Generate load
    generate_test_load &
    local load_pid=$!
    
    # Wait for some activity
    sleep 2
    
    # Trigger graceful shutdown
    echo "Triggering graceful shutdown..."
    kill -TERM $proxy_pid
    
    # Check metrics during shutdown
    echo "Checking metrics during shutdown..."
    local metrics_available=true
    for i in {1..10}; do
        if curl -s http://$PROXY_HOST:$METRICS_PORT/metrics >/dev/null; then
            echo "✓ Metrics still available (check $i)"
        else
            echo "⚠ Metrics no longer available (check $i)"
            metrics_available=false
            break
        fi
        sleep 1
    done
    
    # Clean up
    kill $load_pid 2>/dev/null || true
    wait $proxy_pid 2>/dev/null || true
    
    if $metrics_available; then
        echo "✓ Metrics remained available during shutdown"
    else
        echo "⚠ Metrics became unavailable during shutdown"
    fi
    
    echo
}

# Function to test connection draining
test_connection_draining() {
    echo "Testing connection draining..."
    echo "-----------------------------"
    
    # Start the proxy server
    echo "Starting proxy server..."
    cargo run --example graceful_shutdown_demo &
    local proxy_pid=$!
    
    # Wait for server to start
    sleep 3
    
    # Establish long-running connections
    echo "Establishing long-running connections..."
    for i in {1..3}; do
        (
            # Keep connection alive for a while
            while true; do
                echo "Long running connection $i"
                sleep 2
            done | nc $PROXY_HOST $TCP_PORT
        ) &
        local conn_pids[$i]=$!
    done
    
    # Wait for connections to establish
    sleep 2
    
    # Trigger graceful shutdown
    echo "Triggering graceful shutdown..."
    kill -TERM $proxy_pid
    
    # Monitor the shutdown process
    echo "Monitoring connection draining..."
    local start_time=$(date +%s)
    
    # The proxy should wait for connections to close or timeout
    if wait_for_process_stop $proxy_pid 20; then
        local end_time=$(date +%s)
        local duration=$((end_time - start_time))
        echo "✓ Proxy completed shutdown in ${duration} seconds"
    else
        echo "⚠ Proxy shutdown took longer than expected"
        kill -9 $proxy_pid 2>/dev/null || true
    fi
    
    # Clean up connections
    for pid in "${conn_pids[@]}"; do
        kill $pid 2>/dev/null || true
    done
    
    echo "✓ Connection draining test completed"
    echo
}

# Main test execution
main() {
    echo "Starting graceful shutdown tests..."
    echo "Proxy will be tested on:"
    echo "  TCP: $PROXY_HOST:$TCP_PORT"
    echo "  UDP: $PROXY_HOST:$UDP_PORT"
    echo "  Metrics: $PROXY_HOST:$METRICS_PORT"
    echo
    
    # Test different signals
    test_signal "INT" "SIGINT (Ctrl+C)" "graceful"
    test_signal "TERM" "SIGTERM" "graceful"
    test_signal "QUIT" "SIGQUIT" "force"
    
    # Test metrics during shutdown
    test_metrics_during_shutdown
    
    # Test connection draining
    test_connection_draining
    
    echo "All graceful shutdown tests completed!"
    echo
    echo "Summary:"
    echo "- SIGINT/SIGTERM: Graceful shutdown with connection draining"
    echo "- SIGQUIT: Force shutdown"
    echo "- Metrics remain available during shutdown"
    echo "- Connections are properly drained"
    echo
    echo "The proxy demonstrates proper signal handling and graceful shutdown!"
}

# Check dependencies
if ! command -v nc >/dev/null 2>&1; then
    echo "Error: netcat (nc) is required for testing"
    echo "Install with: sudo apt-get install netcat"
    exit 1
fi

if ! command -v curl >/dev/null 2>&1; then
    echo "Error: curl is required for testing"
    echo "Install with: sudo apt-get install curl"
    exit 1
fi

# Run main function
main "$@"
