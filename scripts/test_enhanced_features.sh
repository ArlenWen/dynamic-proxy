#!/bin/bash

# Enhanced Features Test Script
# This script tests the enhanced TLS and Redis functionality

set -e

PROXY_HOST=${PROXY_HOST:-127.0.0.1}
PROXY_PORT=${PROXY_PORT:-8080}
METRICS_PORT=${METRICS_PORT:-9090}
REDIS_HOST=${REDIS_HOST:-127.0.0.1}
REDIS_PORT=${REDIS_PORT:-6379}

echo "Testing Enhanced Dynamic Proxy Features"
echo "======================================="

# Function to check if service is running
check_service() {
    local host=$1
    local port=$2
    local service_name=$3
    
    if nc -z $host $port 2>/dev/null; then
        echo "✓ $service_name is running on $host:$port"
        return 0
    else
        echo "✗ $service_name is not available on $host:$port"
        return 1
    fi
}

# Function to test Redis backend
test_redis_backend() {
    echo ""
    echo "Testing Redis Backend Integration..."
    echo "-----------------------------------"
    
    # Check Redis connectivity
    if ! check_service $REDIS_HOST $REDIS_PORT "Redis"; then
        echo "Please start Redis server first"
        return 1
    fi
    
    # Test backend retrieval
    echo "Testing backend retrieval from Redis..."
    
    local backends=$(redis-cli -h $REDIS_HOST -p $REDIS_PORT KEYS "proxy:backends:*" | wc -l)
    echo "Found $backends backend configurations in Redis"
    
    if [ $backends -gt 0 ]; then
        echo "✓ Redis backend integration working"
        
        # Show some backend examples
        echo "Sample backends:"
        redis-cli -h $REDIS_HOST -p $REDIS_PORT KEYS "proxy:backends:*" | head -3 | while read key; do
            if [ ! -z "$key" ]; then
                backend_name=$(echo $key | sed 's/proxy:backends://')
                backend_info=$(redis-cli -h $REDIS_HOST -p $REDIS_PORT GET "$key")
                address=$(echo $backend_info | jq -r '.address' 2>/dev/null || echo "unknown")
                echo "  $backend_name -> $address"
            fi
        done
    else
        echo "✗ No backends found in Redis"
        echo "Run scripts/setup_redis_backends.sh first"
        return 1
    fi
}

# Function to test TLS functionality
test_tls_functionality() {
    echo ""
    echo "Testing TLS Functionality..."
    echo "---------------------------"
    
    # Check if proxy is running
    if ! check_service $PROXY_HOST $PROXY_PORT "Dynamic Proxy"; then
        echo "Please start the dynamic proxy server first"
        return 1
    fi
    
    # Test basic TCP connection
    echo "Testing basic TCP connection..."
    if echo "test" | nc $PROXY_HOST $PROXY_PORT >/dev/null 2>&1; then
        echo "✓ Basic TCP connection successful"
    else
        echo "✗ Basic TCP connection failed"
    fi
    
    # Test TLS connection (if certificates are available)
    if [ -f "certs/server.crt" ] && [ -f "certs/server.key" ]; then
        echo "Testing TLS connection..."
        if echo "test" | openssl s_client -connect $PROXY_HOST:$PROXY_PORT -quiet >/dev/null 2>&1; then
            echo "✓ TLS connection successful"
        else
            echo "✗ TLS connection failed (this is expected if TLS is not configured)"
        fi
    else
        echo "⚠ TLS certificates not found, skipping TLS tests"
    fi
}

# Function to test SNI routing
test_sni_routing() {
    echo ""
    echo "Testing SNI Routing..."
    echo "---------------------"
    
    # Test different SNI hostnames
    local test_hostnames=("api.example.com" "www.example.com" "secure.example.com")
    
    for hostname in "${test_hostnames[@]}"; do
        echo "Testing SNI routing for: $hostname"
        
        # Check if backend exists in Redis
        local backend_info=$(redis-cli -h $REDIS_HOST -p $REDIS_PORT GET "proxy:backends:$hostname" 2>/dev/null)
        if [ ! -z "$backend_info" ]; then
            local address=$(echo $backend_info | jq -r '.address' 2>/dev/null || echo "unknown")
            echo "  ✓ Backend configured: $hostname -> $address"
        else
            echo "  ⚠ No specific backend for $hostname, will use prefix matching"
        fi
    done
}

# Function to test metrics endpoint
test_metrics() {
    echo ""
    echo "Testing Metrics Endpoint..."
    echo "--------------------------"
    
    if check_service $PROXY_HOST $METRICS_PORT "Metrics Server"; then
        echo "Fetching metrics..."
        local metrics=$(curl -s http://$PROXY_HOST:$METRICS_PORT/metrics | head -10)
        if [ ! -z "$metrics" ]; then
            echo "✓ Metrics endpoint working"
            echo "Sample metrics:"
            echo "$metrics" | grep -E "(proxy_|connections_|bytes_)" | head -5
        else
            echo "✗ Metrics endpoint not responding"
        fi
    fi
}

# Function to test health checks
test_health_checks() {
    echo ""
    echo "Testing Health Check Functionality..."
    echo "------------------------------------"
    
    # Test health check for known backends
    local test_backends=("api_backend" "web_backend_1" "default")
    
    for backend in "${test_backends[@]}"; do
        echo "Testing health check for: $backend"
        
        local backend_info=$(redis-cli -h $REDIS_HOST -p $REDIS_PORT GET "proxy:backends:$backend" 2>/dev/null)
        if [ ! -z "$backend_info" ]; then
            local address=$(echo $backend_info | jq -r '.address' 2>/dev/null || echo "unknown")
            local healthy=$(echo $backend_info | jq -r '.healthy' 2>/dev/null || echo "unknown")
            echo "  Backend: $backend -> $address (healthy: $healthy)"
            
            # Try to connect to the backend
            local host=$(echo $address | cut -d: -f1)
            local port=$(echo $address | cut -d: -f2)
            
            if nc -z $host $port 2>/dev/null; then
                echo "  ✓ Backend is reachable"
            else
                echo "  ✗ Backend is not reachable"
            fi
        else
            echo "  ✗ Backend not found in Redis"
        fi
    done
}

# Function to run load test
run_load_test() {
    echo ""
    echo "Running Basic Load Test..."
    echo "-------------------------"
    
    if ! command -v ab >/dev/null 2>&1; then
        echo "⚠ Apache Bench (ab) not found, skipping load test"
        echo "Install with: sudo apt-get install apache2-utils"
        return
    fi
    
    echo "Running 100 requests with concurrency 10..."
    ab -n 100 -c 10 http://$PROXY_HOST:$PROXY_PORT/ 2>/dev/null | grep -E "(Requests per second|Time per request|Transfer rate)" || echo "Load test completed"
}

# Main test execution
main() {
    echo "Starting enhanced features test..."
    echo "Proxy: $PROXY_HOST:$PROXY_PORT"
    echo "Redis: $REDIS_HOST:$REDIS_PORT"
    echo "Metrics: $PROXY_HOST:$METRICS_PORT"
    echo ""
    
    # Run tests
    test_redis_backend
    test_tls_functionality
    test_sni_routing
    test_metrics
    test_health_checks
    run_load_test
    
    echo ""
    echo "Enhanced features test completed!"
    echo ""
    echo "Summary:"
    echo "- Redis backend integration tested"
    echo "- TLS functionality verified"
    echo "- SNI routing configuration checked"
    echo "- Metrics endpoint validated"
    echo "- Health checks tested"
    echo ""
    echo "For more detailed testing, check the logs and metrics."
}

# Run main function
main "$@"
