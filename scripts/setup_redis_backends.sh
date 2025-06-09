#!/bin/bash

# Redis Backend Setup Script
# This script initializes Redis with backend server configurations

REDIS_HOST=${REDIS_HOST:-127.0.0.1}
REDIS_PORT=${REDIS_PORT:-6379}
REDIS_URL="redis://${REDIS_HOST}:${REDIS_PORT}"

echo "Setting up Redis backends at ${REDIS_URL}..."

# Check if Redis is available
if ! redis-cli -h $REDIS_HOST -p $REDIS_PORT ping > /dev/null 2>&1; then
    echo "Error: Redis server is not available at ${REDIS_HOST}:${REDIS_PORT}"
    echo "Please start Redis server first."
    exit 1
fi

echo "Redis server is available. Setting up backend configurations..."

# Function to add backend to Redis
add_backend() {
    local name=$1
    local address=$2
    local weight=${3:-100}
    local healthy=${4:-true}
    
    local backend_json=$(cat <<EOF
{
    "address": "$address",
    "weight": $weight,
    "healthy": $healthy,
    "last_check": $(date +%s),
    "metadata": {}
}
EOF
)
    
    redis-cli -h $REDIS_HOST -p $REDIS_PORT SET "proxy:backends:$name" "$backend_json"
    echo "Added backend: $name -> $address"
}

# Add example backend configurations
# Note: These are example configurations. Modify the addresses and names
# according to your actual backend services.
echo "Adding example backend configurations..."

add_backend "api_backend" "127.0.0.1:8001" 100 true
add_backend "web_backend_1" "127.0.0.1:8002" 100 true
add_backend "web_backend_2" "127.0.0.1:8003" 100 true
add_backend "secure_backend" "127.0.0.1:8004" 100 true
add_backend "admin_backend" "127.0.0.1:8005" 100 true
add_backend "default" "127.0.0.1:8000" 100 true

# Add SNI-specific backends (examples)
# Note: Replace example.com with your actual domain names
echo "Adding SNI-specific backend configurations..."

add_backend "api.example.com" "127.0.0.1:8001" 100 true
add_backend "www.example.com" "127.0.0.1:8002" 100 true
add_backend "secure.example.com" "127.0.0.1:8004" 100 true
add_backend "admin.example.com" "127.0.0.1:8005" 100 true

# Add wildcard backends
add_backend "*.example.com" "127.0.0.1:8002" 50 true
add_backend "*.api.example.com" "127.0.0.1:8001" 80 true

# Add load balancing backends
echo "Adding load balancing backend configurations..."

add_backend "lb_web_1" "127.0.0.1:8010" 100 true
add_backend "lb_web_2" "127.0.0.1:8011" 100 true
add_backend "lb_web_3" "127.0.0.1:8012" 80 true

# Add some unhealthy backends for testing
add_backend "unhealthy_backend" "127.0.0.1:9999" 100 false

echo "Backend setup completed!"

# List all configured backends
echo ""
echo "Configured backends:"
redis-cli -h $REDIS_HOST -p $REDIS_PORT KEYS "proxy:backends:*" | while read key; do
    if [ ! -z "$key" ]; then
        backend_name=$(echo $key | sed 's/proxy:backends://')
        backend_info=$(redis-cli -h $REDIS_HOST -p $REDIS_PORT GET "$key")
        address=$(echo $backend_info | jq -r '.address')
        healthy=$(echo $backend_info | jq -r '.healthy')
        weight=$(echo $backend_info | jq -r '.weight')
        echo "  $backend_name: $address (weight: $weight, healthy: $healthy)"
    fi
done

echo ""
echo "Redis backend setup completed successfully!"
echo "You can now start the dynamic proxy server."
