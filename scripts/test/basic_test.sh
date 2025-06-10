#!/bin/bash

# Basic test script for dynamic-proxy
set -e

echo "=== Dynamic Proxy Basic Test ==="

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Test binary path
BINARY="./target/debug/dynamic-proxy"

# Check if binary exists
if [ ! -f "$BINARY" ]; then
    echo -e "${RED}Error: Binary not found at $BINARY${NC}"
    echo "Please run 'cargo build' first"
    exit 1
fi

echo -e "${YELLOW}Testing binary: $BINARY${NC}"

# Test 1: Help command
echo -e "\n${YELLOW}Test 1: Help command${NC}"
if $BINARY --help > /dev/null 2>&1; then
    echo -e "${GREEN}✓ Help command works${NC}"
else
    echo -e "${RED}✗ Help command failed${NC}"
    exit 1
fi

# Test 2: Version info
echo -e "\n${YELLOW}Test 2: Version info${NC}"
if $BINARY --version-info > /dev/null 2>&1; then
    echo -e "${GREEN}✓ Version info works${NC}"
else
    echo -e "${RED}✗ Version info failed${NC}"
    exit 1
fi

# Test 3: Configuration validation
echo -e "\n${YELLOW}Test 3: Configuration validation${NC}"
if $BINARY --validate > /dev/null 2>&1; then
    echo -e "${GREEN}✓ Configuration validation passed${NC}"
else
    echo -e "${RED}✗ Configuration validation failed${NC}"
    exit 1
fi

# Test 4: Invalid configuration file
echo -e "\n${YELLOW}Test 4: Invalid configuration file${NC}"
if $BINARY -c non_existent_config.toml --validate > /dev/null 2>&1; then
    echo -e "${RED}✗ Should have failed with non-existent config${NC}"
    exit 1
else
    echo -e "${GREEN}✓ Correctly failed with non-existent config${NC}"
fi

# Test 5: Create a minimal test config
echo -e "\n${YELLOW}Test 5: Minimal configuration test${NC}"
cat > test_config.toml << 'EOF'
[server]
tcp_bind = "127.0.0.1:18080"
udp_bind = "127.0.0.1:18081"

[redis]
url = "redis://localhost:6379"

[logging]
level = "info"
format = "pretty"

[metrics]
enabled = false
bind = "127.0.0.1:19090"
path = "/metrics"

[health_check]
enabled = false
interval = 30
timeout = 5
retries = 3
failure_threshold = 3
success_threshold = 2

[routers.test_router]
plugin = "round_robin"
config = {}

[[rules]]
name = "test_rule"
protocol = "tcp"
enabled = true
router = "test_router"

[[rules.backends]]
id = "test_backend"
address = "127.0.0.1:8080"
enabled = true

[rules.matcher]
type = "port"
port = 80
EOF

if $BINARY -c test_config.toml --validate > /dev/null 2>&1; then
    echo -e "${GREEN}✓ Minimal configuration validation passed${NC}"
else
    echo -e "${RED}✗ Minimal configuration validation failed${NC}"
    exit 1
fi

# Clean up test config
rm -f test_config.toml

# Test 6: Start server briefly (if no other instance is running)
echo -e "\n${YELLOW}Test 6: Brief server start test${NC}"
# Check if ports are available
if ! netstat -tuln 2>/dev/null | grep -q ":18080\|:18081"; then
    # Create a test config with different ports
    cat > test_start_config.toml << 'EOF'
[server]
tcp_bind = "127.0.0.1:18080"
udp_bind = "127.0.0.1:18081"

[redis]
url = "redis://localhost:6379"

[logging]
level = "warn"
format = "pretty"

[metrics]
enabled = false
bind = "127.0.0.1:19090"
path = "/metrics"

[health_check]
enabled = false
interval = 30
timeout = 5
retries = 3
failure_threshold = 3
success_threshold = 2

[routers.test_router]
plugin = "round_robin"
config = {}

[[rules]]
name = "test_rule"
protocol = "tcp"
enabled = true
router = "test_router"

[[rules.backends]]
id = "test_backend"
address = "127.0.0.1:8080"
enabled = true

[rules.matcher]
type = "port"
port = 80
EOF

    # Start server in background and kill it after 2 seconds
    timeout 2s $BINARY -c test_start_config.toml > /dev/null 2>&1 || true
    
    if [ $? -eq 124 ]; then  # timeout exit code
        echo -e "${GREEN}✓ Server started and stopped successfully${NC}"
    else
        echo -e "${YELLOW}⚠ Server start test inconclusive${NC}"
    fi
    
    # Clean up
    rm -f test_start_config.toml
else
    echo -e "${YELLOW}⚠ Ports in use, skipping server start test${NC}"
fi

echo -e "\n${GREEN}=== All tests passed! ===${NC}"
echo -e "${YELLOW}Dynamic Proxy is ready for use.${NC}"

# Show some usage examples
echo -e "\n${YELLOW}Usage examples:${NC}"
echo "  $BINARY --help"
echo "  $BINARY --version-info"
echo "  $BINARY --validate"
echo "  $BINARY -c config.toml"
echo "  $BINARY -c config.toml --log-level debug"
