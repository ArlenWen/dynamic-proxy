#!/bin/bash

# Main Demo Script for Dynamic Proxy
# This script provides a menu to run different demos and tests

echo "Dynamic Proxy - Demo Menu"
echo "========================"
echo
echo "Available demos and tests:"
echo "  1) Basic Demo          - Simple proxy without Redis/TLS"
echo "  2) Enhanced Demo       - Full-featured proxy with Redis/TLS"
echo "  3) Graceful Shutdown   - Test graceful shutdown functionality"
echo "  4) Test Client         - Run test client to verify proxy"
echo "  5) Enhanced Features   - Run comprehensive feature tests"
echo "  6) Graceful Shutdown   - Run graceful shutdown tests"
echo "  7) Setup Redis         - Initialize Redis with example backends"
echo "  q) Quit"
echo

# Change to project root directory
cd "$(dirname "$0")/.."

while true; do
    echo -n "Select an option [1-7, q]: "
    read choice
    
    case $choice in
        1)
            echo
            echo "Starting Basic Demo..."
            bash scripts/demo/run_basic_demo.sh
            ;;
        2)
            echo
            echo "Starting Enhanced Demo..."
            bash scripts/demo/run_enhanced_demo.sh
            ;;
        3)
            echo
            echo "Starting Graceful Shutdown Demo..."
            bash scripts/demo/run_graceful_shutdown_demo.sh
            ;;
        4)
            echo
            echo "Running Test Client..."
            bash scripts/test/run_test_client.sh
            ;;
        5)
            echo
            echo "Running Enhanced Features Tests..."
            bash scripts/test_enhanced_features.sh
            ;;
        6)
            echo
            echo "Running Graceful Shutdown Tests..."
            bash scripts/test_graceful_shutdown.sh
            ;;
        7)
            echo
            echo "Setting up Redis backends..."
            bash scripts/setup_redis_backends.sh
            ;;
        q|Q)
            echo "Goodbye!"
            exit 0
            ;;
        *)
            echo "Invalid option. Please select 1-7 or q."
            ;;
    esac
    
    echo
    echo "Press Enter to return to menu..."
    read
    echo
done
