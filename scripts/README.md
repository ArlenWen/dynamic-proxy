# Scripts Directory

This directory contains various scripts for running demos, tests, and managing the Dynamic Proxy project.

## Quick Start

For the best experience, use the main demo menu:

```bash
bash scripts/demo.sh
```

## Directory Structure

```
scripts/
├── demo.sh                     # Main demo menu (recommended)
├── demo/                       # Demo scripts
│   ├── run_basic_demo.sh      # Basic proxy demo (no Redis/TLS)
│   ├── run_enhanced_demo.sh   # Full-featured demo (Redis + TLS)
│   └── run_graceful_shutdown_demo.sh # Graceful shutdown demo
├── test/                       # Test scripts
│   ├── run_test_client.sh     # Test client for proxy verification
│   └── test_fixes.sh          # Fix verification tests
├── setup_redis_backends.sh    # Redis backend initialization
├── test_enhanced_features.sh  # Comprehensive feature tests
└── test_graceful_shutdown.sh  # Graceful shutdown tests
```

## Available Scripts

### Demo Scripts

- **`demo.sh`** - Interactive menu for all demos and tests
- **`demo/run_basic_demo.sh`** - Simple proxy without Redis or TLS
- **`demo/run_enhanced_demo.sh`** - Full-featured proxy with Redis and TLS
- **`demo/run_graceful_shutdown_demo.sh`** - Demonstrates graceful shutdown

### Test Scripts

- **`test/run_test_client.sh`** - Test client to verify proxy functionality
- **`test/test_fixes.sh`** - Verify all code fixes and compilation
- **`test_enhanced_features.sh`** - Comprehensive feature testing
- **`test_graceful_shutdown.sh`** - Automated graceful shutdown testing

### Setup Scripts

- **`setup_redis_backends.sh`** - Initialize Redis with example backend configurations

## Usage Examples

### Run Basic Demo
```bash
bash scripts/demo/run_basic_demo.sh
```

### Run Enhanced Demo (requires Redis)
```bash
# First setup Redis backends
bash scripts/setup_redis_backends.sh

# Then run the demo
bash scripts/demo/run_enhanced_demo.sh
```

### Test Proxy Functionality
```bash
# Start a demo in one terminal
bash scripts/demo/run_basic_demo.sh

# Test in another terminal
bash scripts/test/run_test_client.sh
```

### Verify Code Fixes
```bash
bash scripts/test/test_fixes.sh
```

### Run Comprehensive Tests
```bash
bash scripts/test_enhanced_features.sh
```

## Prerequisites

### For Basic Demo
- Rust and Cargo installed
- No additional dependencies

### For Enhanced Demo
- Redis server running on `127.0.0.1:6379`
- TLS certificates in `certs/` directory (optional)

### For Tests
- Running proxy instance (for client tests)
- Redis server (for enhanced feature tests)

## Migration Notice

The old scripts in the project root (`run_demo.sh` and `test_proxy.sh`) have been updated to redirect to the new organized structure. They will continue to work but will show a deprecation notice.

For the best experience, use the new organized scripts in this directory.
