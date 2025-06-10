#!/bin/bash

# Demo script for dynamic-proxy
set -e

echo "=== Dynamic Proxy Demo ==="

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration
BINARY="./target/debug/dynamic-proxy"
DEMO_CONFIG="demo_config.toml"
TCP_PORT=18080
UDP_PORT=18081
METRICS_PORT=19090

# Check if binary exists
if [ ! -f "$BINARY" ]; then
    echo -e "${RED}Error: Binary not found at $BINARY${NC}"
    echo "Please run 'cargo build' first"
    exit 1
fi

# Function to cleanup
cleanup() {
    echo -e "\n${YELLOW}Cleaning up...${NC}"
    # Kill any running proxy instances
    pkill -f "dynamic-proxy.*$DEMO_CONFIG" 2>/dev/null || true
    # Remove demo config
    rm -f "$DEMO_CONFIG"
    echo -e "${GREEN}Cleanup completed${NC}"
}

# Set trap for cleanup
trap cleanup EXIT

# Create demo configuration
echo -e "${BLUE}Creating demo configuration...${NC}"
cat > "$DEMO_CONFIG" << EOF
# Demo Configuration for Dynamic Proxy

[server]
tcp_bind = "127.0.0.1:$TCP_PORT"
udp_bind = "127.0.0.1:$UDP_PORT"
worker_threads = 2
max_connections = 1000
connection_timeout = 30
buffer_size = 8192

[redis]
url = "redis://localhost:6379"
password = "demo_password"
db = 0
pool_size = 5
connection_timeout = 5
command_timeout = 3

[logging]
level = "info"
format = "pretty"

[metrics]
enabled = true
bind = "127.0.0.1:$METRICS_PORT"
path = "/metrics"
collect_interval = 10

[health_check]
enabled = true
interval = 30
timeout = 5
retries = 3
failure_threshold = 3
success_threshold = 2

# Router configurations
[routers.demo_round_robin]
plugin = "round_robin"
config = {}

[routers.demo_weighted]
plugin = "weighted"
config = { smooth_weighted = true }

[routers.demo_hash]
plugin = "hash"
config = { hash_key = "client_ip" }

[routers.demo_least_conn]
plugin = "least_connections"
config = {}

[routers.demo_random]
plugin = "random"
config = {}

# Demo routing rules
[[rules]]
name = "web_demo"
protocol = "tcp"
enabled = true
router = "demo_round_robin"

[[rules.backends]]
id = "web1"
address = "127.0.0.1:8001"
weight = 1
enabled = true

[[rules.backends]]
id = "web2"
address = "127.0.0.1:8002"
weight = 1
enabled = true

[[rules.backends]]
id = "web3"
address = "127.0.0.1:8003"
weight = 2
enabled = true

[rules.matcher]
type = "port"
port = 80

[[rules]]
name = "api_demo"
protocol = "tcp"
enabled = true
router = "demo_weighted"

[[rules.backends]]
id = "api1"
address = "127.0.0.1:9001"
weight = 3
enabled = true

[[rules.backends]]
id = "api2"
address = "127.0.0.1:9002"
weight = 2
enabled = true

[rules.matcher]
type = "port_range"
start = 8000
end = 8999

[[rules]]
name = "dns_demo"
protocol = "udp"
enabled = true
router = "demo_hash"

[[rules.backends]]
id = "dns1"
address = "8.8.8.8:53"
weight = 1
enabled = true

[[rules.backends]]
id = "dns2"
address = "8.8.4.4:53"
weight = 1
enabled = true

[rules.matcher]
type = "port"
port = 53
EOF

echo -e "${GREEN}Demo configuration created: $DEMO_CONFIG${NC}"

# Validate configuration
echo -e "\n${BLUE}Validating configuration...${NC}"
if $BINARY -c "$DEMO_CONFIG" --validate; then
    echo -e "${GREEN}Configuration is valid!${NC}"
else
    echo -e "${RED}Configuration validation failed!${NC}"
    exit 1
fi

# Show configuration details
echo -e "\n${BLUE}Demo Configuration Details:${NC}"
echo -e "  TCP Proxy: ${YELLOW}127.0.0.1:$TCP_PORT${NC}"
echo -e "  UDP Proxy: ${YELLOW}127.0.0.1:$UDP_PORT${NC}"
echo -e "  Metrics:   ${YELLOW}http://127.0.0.1:$METRICS_PORT/metrics${NC}"
echo -e "  Health:    ${YELLOW}http://127.0.0.1:$METRICS_PORT/health${NC}"

echo -e "\n${BLUE}Routing Rules:${NC}"
echo -e "  ${YELLOW}web_demo${NC}:  TCP port 80 → Round Robin (web1, web2, web3)"
echo -e "  ${YELLOW}api_demo${NC}:  TCP ports 8000-8999 → Weighted (api1:3, api2:2)"
echo -e "  ${YELLOW}dns_demo${NC}:  UDP port 53 → Hash by client IP (8.8.8.8, 8.8.4.4)"

echo -e "\n${BLUE}Available Router Plugins:${NC}"
echo -e "  • ${YELLOW}round_robin${NC}: Distributes requests evenly across backends"
echo -e "  • ${YELLOW}weighted${NC}: Routes based on backend weights"
echo -e "  • ${YELLOW}hash${NC}: Consistent hashing for session affinity"
echo -e "  • ${YELLOW}least_connections${NC}: Routes to backend with fewest connections"
echo -e "  • ${YELLOW}random${NC}: Random backend selection"

# Ask user if they want to start the server
echo -e "\n${YELLOW}Would you like to start the demo server? (y/N)${NC}"
read -r response

if [[ "$response" =~ ^[Yy]$ ]]; then
    echo -e "\n${BLUE}Starting Dynamic Proxy demo server...${NC}"
    echo -e "${YELLOW}Press Ctrl+C to stop the server${NC}"
    
    # Check if ports are available
    if netstat -tuln 2>/dev/null | grep -q ":$TCP_PORT\|:$UDP_PORT\|:$METRICS_PORT"; then
        echo -e "${RED}Warning: Some demo ports are already in use!${NC}"
        echo -e "Please stop other services using ports $TCP_PORT, $UDP_PORT, or $METRICS_PORT"
        exit 1
    fi
    
    # Start the server
    echo -e "\n${GREEN}Server starting...${NC}"
    echo -e "Logs will appear below. Use Ctrl+C to stop.\n"
    
    # Run the proxy
    $BINARY -c "$DEMO_CONFIG" --log-level info
    
else
    echo -e "\n${BLUE}Demo configuration is ready!${NC}"
    echo -e "To start the server manually, run:"
    echo -e "  ${YELLOW}$BINARY -c $DEMO_CONFIG${NC}"
    echo -e "\nTo test the configuration:"
    echo -e "  ${YELLOW}$BINARY -c $DEMO_CONFIG --validate${NC}"
    echo -e "\nTo view metrics (when server is running):"
    echo -e "  ${YELLOW}curl http://127.0.0.1:$METRICS_PORT/metrics${NC}"
    echo -e "  ${YELLOW}curl http://127.0.0.1:$METRICS_PORT/health${NC}"
fi
