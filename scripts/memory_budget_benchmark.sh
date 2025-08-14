#!/bin/bash
#
# Memory Budget Validation Benchmark
# 
# Focused testing of memory budget system effectiveness and performance impact

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "$PROJECT_ROOT"

# Colors
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

echo -e "${BLUE}=== Memory Budget Validation Benchmark ===${NC}"

# Create test file
BENCHMARK_DIR="$PROJECT_ROOT/tmp/memory_benchmark"
mkdir -p "$BENCHMARK_DIR"

# Generate a controlled test file
generate_test_file() {
    local filename="$BENCHMARK_DIR/memory_test.log"
    local lines=50000  # 50k lines = ~10MB file
    
    echo -e "${YELLOW}Generating test file with $lines lines...${NC}"
    
    {
        for i in $(seq 1 $lines); do
            local timestamp=$(date -u +"%Y-%m-%dT%H:%M:%S.%3NZ")
            local level=$(shuf -n1 -e "INFO" "WARN" "ERROR" "DEBUG")
            
            # Alternate between canonical and message formats
            if [ $((i % 2)) -eq 0 ]; then
                echo "{\"timestamp\":\"$timestamp\",\"level\":\"$level\",\"message\":\"Test message $i for memory budget validation\",\"method\":\"GET\",\"url\":\"/api/test/$i\",\"status\":200,\"elapsed\":\"${i}ms\",\"size\":$((i * 10)),\"request_id\":\"req_$i\",\"remote_host\":\"192.168.1.100\",\"user_agent\":\"MemoryTestClient/1.0\"}"
            else
                echo "{\"timestamp\":\"$timestamp\",\"level\":\"$level\",\"message\":\"Flexible message $i\",\"request_id\":\"msg_$i\",\"module\":\"memory_test\"}"
            fi
        done
    } > "$filename"
    
    echo -e "${GREEN}Generated test file: $(du -h "$filename")${NC}"
}

# Test memory budget effectiveness
test_memory_limits() {
    local test_file="$BENCHMARK_DIR/memory_test.log"
    
    echo -e "\n${BLUE}=== Testing Memory Budget Limits ===${NC}"
    
    # Test different memory limits
    local limits=("5MB" "10MB" "25MB" "50MB")
    
    for limit in "${limits[@]}"; do
        echo -e "\n${YELLOW}--- Testing with $limit memory limit ---${NC}"
        
        local limit_bytes
        case $limit in
            *MB) limit_bytes=$((${limit%MB} * 1024 * 1024)) ;;
            *) limit_bytes=$limit ;;
        esac
        
        echo "Memory limit: $limit ($limit_bytes bytes)"
        echo "Command: cargo run --release -- --max-memory $limit_bytes --chunked $test_file"
        
        # Run with timing
        time cargo run --release -- --max-memory "$limit_bytes" --chunked "$test_file" >/dev/null
        
        echo ""
    done
}

# Compare memory budget vs no limit
test_budget_impact() {
    local test_file="$BENCHMARK_DIR/memory_test.log"
    
    echo -e "\n${BLUE}=== Memory Budget Performance Impact ===${NC}"
    
    echo -e "\n${YELLOW}--- Without Memory Budget (High Limit) ---${NC}"
    echo "Command: cargo run --release -- --max-memory 500000000 --chunked $test_file"
    time cargo run --release -- --max-memory 500000000 --chunked "$test_file" >/dev/null
    
    echo -e "\n${YELLOW}--- With Memory Budget (Constrained) ---${NC}"  
    echo "Command: cargo run --release -- --max-memory 10485760 --chunked $test_file"
    time cargo run --release -- --max-memory 10485760 --chunked "$test_file" >/dev/null
    
    echo -e "\n${YELLOW}--- With Memory Budget (Very Constrained) ---${NC}"
    echo "Command: cargo run --release -- --max-memory 2097152 --chunked $test_file"
    time cargo run --release -- --max-memory 2097152 --chunked "$test_file" >/dev/null
}

# Test strategy behavior under memory pressure
test_strategy_adaptation() {
    local test_file="$BENCHMARK_DIR/memory_test.log"
    
    echo -e "\n${BLUE}=== Strategy Adaptation Under Memory Pressure ===${NC}"
    
    local strategies=("static" "adaptive" "conservative")
    local memory_limit="5242880"  # 5MB
    
    for strategy in "${strategies[@]}"; do
        echo -e "\n${YELLOW}--- Testing $strategy strategy with 5MB limit ---${NC}"
        echo "Command: cargo run --release -- --max-memory $memory_limit --chunk-strategy $strategy --chunked $test_file"
        
        time cargo run --release -- --max-memory "$memory_limit" --chunk-strategy "$strategy" --chunked "$test_file" >/dev/null
        
        echo ""
    done
}

# Test emergency allocation behavior
test_emergency_scenarios() {
    local test_file="$BENCHMARK_DIR/memory_test.log"
    
    echo -e "\n${BLUE}=== Emergency Allocation Scenarios ===${NC}"
    
    echo -e "\n${YELLOW}--- Very Low Memory (2MB) - Should Trigger Emergency ---${NC}"
    echo "Command: cargo run --release -- --max-memory 2097152 --chunked $test_file"
    
    # Capture stderr to see emergency messages
    cargo run --release -- --max-memory 2097152 --chunked "$test_file" 2>&1 | head -20
    
    echo -e "\n${YELLOW}--- Ultra Low Memory (1MB) - May Fail ---${NC}"
    echo "Command: cargo run --release -- --max-memory 1048576 --chunked $test_file"
    
    # This might fail, so don't exit on error
    set +e
    cargo run --release -- --max-memory 1048576 --chunked "$test_file" 2>&1 | head -20
    local exit_code=$?
    set -e
    
    if [ $exit_code -ne 0 ]; then
        echo -e "${RED}Failed as expected with ultra-low memory${NC}"
    else
        echo -e "${GREEN}Surprisingly succeeded with ultra-low memory${NC}"
    fi
}

# Main execution
main() {
    echo "Starting memory budget validation benchmark..."
    echo ""
    
    # Build release binary
    echo -e "${YELLOW}Building release binary...${NC}"
    cargo build --release
    echo ""
    
    # Generate test file
    generate_test_file
    
    # Run tests
    test_memory_limits
    test_budget_impact
    test_strategy_adaptation
    test_emergency_scenarios
    
    # Cleanup
    echo -e "\n${YELLOW}Cleaning up...${NC}"
    rm -rf "$BENCHMARK_DIR"
    
    echo -e "\n${GREEN}Memory budget validation benchmark complete!${NC}"
    echo ""
    echo "Key observations to look for:"
    echo "- Memory pressure warnings (⚠️) when limits are tight"
    echo "- Emergency allocation messages (🆘) under extreme pressure"
    echo "- Performance differences between constrained vs unconstrained memory"
    echo "- Strategy adaptation behavior under memory pressure"
}

# Help function
if [[ "${1:-}" == "--help" || "${1:-}" == "-h" ]]; then
    echo "Memory Budget Validation Benchmark"
    echo ""
    echo "This script tests the memory budget system with various scenarios:"
    echo "  - Different memory limits (5MB to 50MB)"
    echo "  - Performance impact comparison"
    echo "  - Strategy adaptation under pressure"
    echo "  - Emergency allocation scenarios"
    echo ""
    echo "Usage: $0"
    exit 0
fi

main