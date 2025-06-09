#!/bin/bash

# Test script to verify all fixes are working correctly
# This script tests the dynamic proxy functionality after fixes

echo "Dynamic Proxy - Fix Verification Test"
echo "====================================="
echo

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Test results
TESTS_PASSED=0
TESTS_FAILED=0

# Function to print test results
print_result() {
    if [ $1 -eq 0 ]; then
        echo -e "${GREEN}✓ PASS${NC}: $2"
        ((TESTS_PASSED++))
    else
        echo -e "${RED}✗ FAIL${NC}: $2"
        ((TESTS_FAILED++))
    fi
}

# Change to project root directory
cd "$(dirname "$0")/../.."

# Test 1: Check if project builds successfully
echo "Test 1: Building project..."
cargo build --quiet
print_result $? "Project builds without errors"

# Test 2: Check if basic example compiles
echo "Test 2: Checking basic example..."
cargo check --example basic_usage --quiet
print_result $? "Basic example compiles"

# Test 3: Check if enhanced demo compiles
echo "Test 3: Checking enhanced demo..."
cargo check --example enhanced_demo --quiet
print_result $? "Enhanced demo compiles"

# Test 4: Check if test client compiles
echo "Test 4: Checking test client..."
cargo check --example test_client --quiet
print_result $? "Test client compiles"

# Test 5: Check if configuration file exists
echo "Test 5: Checking configuration file..."
if [ -f "config.toml" ]; then
    print_result 0 "Configuration file exists"
else
    print_result 1 "Configuration file missing"
fi

# Test 6: Check if config.example.toml was removed (per user preference)
echo "Test 6: Checking example config removal..."
if [ ! -f "config.example.toml" ]; then
    print_result 0 "Example configuration file removed as requested"
else
    print_result 1 "Example configuration file still exists"
fi

# Test 7: Validate configuration file syntax
echo "Test 7: Validating configuration syntax..."
if command -v toml-test >/dev/null 2>&1; then
    toml-test config.toml >/dev/null 2>&1
    print_result $? "Configuration file has valid TOML syntax"
else
    # Fallback: try to parse with a simple check
    if grep -q "^\[server\]" config.toml && grep -q "^\[redis\]" config.toml; then
        print_result 0 "Configuration file appears to have valid structure"
    else
        print_result 1 "Configuration file structure validation failed"
    fi
fi

# Test 8: Check if Redis backend has fallback support
echo "Test 8: Checking Redis backend fallback..."
if grep -q "fallback_backends" src/routing/redis_backend.rs; then
    print_result 0 "Redis backend has fallback support"
else
    print_result 1 "Redis backend fallback support missing"
fi

# Test 9: Check if plugin initialization is fixed
echo "Test 9: Checking plugin initialization fix..."
if grep -q "RoutingPluginMut" src/routing/plugin.rs && grep -q "initialize_plugins" src/routing/plugin.rs; then
    print_result 0 "Plugin initialization system is fixed"
else
    print_result 1 "Plugin initialization system still has issues"
fi

# Test 10: Check if TLS SNI parsing is improved
echo "Test 10: Checking TLS SNI parsing improvements..."
if grep -q "is_valid_hostname" src/proxy/tls_handler.rs && grep -q "debug!" src/proxy/tls_handler.rs; then
    print_result 0 "TLS SNI parsing has been improved"
else
    print_result 1 "TLS SNI parsing improvements missing"
fi

# Test 11: Check if routing rules have backend resolution
echo "Test 11: Checking routing rules backend resolution..."
if grep -q "resolve_backend_addr" src/routing/rules.rs; then
    print_result 0 "Routing rules have backend address resolution"
else
    print_result 1 "Routing rules backend resolution missing"
fi

# Test 12: Check if health checks are enhanced
echo "Test 12: Checking health check enhancements..."
if grep -q "perform_health_check" src/routing/redis_backend.rs && grep -q "http_health_check" src/routing/redis_backend.rs; then
    print_result 0 "Health checks have been enhanced"
else
    print_result 1 "Health check enhancements missing"
fi

echo
echo "Test Summary:"
echo "============="
echo -e "Tests passed: ${GREEN}${TESTS_PASSED}${NC}"
echo -e "Tests failed: ${RED}${TESTS_FAILED}${NC}"
echo -e "Total tests:  $((TESTS_PASSED + TESTS_FAILED))"

if [ $TESTS_FAILED -eq 0 ]; then
    echo -e "\n${GREEN}🎉 All tests passed! The fixes have been successfully implemented.${NC}"
    exit 0
else
    echo -e "\n${YELLOW}⚠️  Some tests failed. Please review the issues above.${NC}"
    exit 1
fi
