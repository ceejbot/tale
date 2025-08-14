#!/bin/bash
#
# Quick Memory Budget Validation
# A simple test to verify memory budget system is working correctly

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

echo -e "${BLUE}=== Quick Memory Budget Validation ===${NC}"
echo ""

# Create a quick test file
TEST_FILE="/tmp/quick_memory_test.log"
echo -e "${YELLOW}Creating test file...${NC}"

# Generate 20k lines (~4MB file)
{
    for i in $(seq 1 20000); do
        if [ $((i % 2)) -eq 0 ]; then
            # Canonical format  
            echo "{\"timestamp\":\"2025-08-11T10:30:00Z\",\"level\":\"INFO\",\"message\":\"Quick test message $i\",\"method\":\"GET\",\"url\":\"/test/$i\",\"status\":200,\"elapsed\":\"${i}ms\",\"size\":$((i*10)),\"request_id\":\"test_$i\",\"remote_host\":\"127.0.0.1\",\"user_agent\":\"QuickTest/1.0\"}"
        else
            # Message format
            echo "{\"timestamp\":\"2025-08-11T10:30:00Z\",\"level\":\"DEBUG\",\"message\":\"Flexible message $i\",\"request_id\":\"msg_$i\",\"module\":\"quick_test\"}"
        fi
    done
} > "$TEST_FILE"

echo "Generated test file: $(du -h "$TEST_FILE" | cut -f1)"
echo ""

# Test different memory configurations
echo -e "${BLUE}=== Memory Budget Tests ===${NC}"

# Test 1: Generous memory (should be fast)
echo -e "\n${YELLOW}Test 1: Generous Memory (50MB)${NC}"
echo "Command: cargo run --release -- --max-memory 52428800 --chunked $TEST_FILE"
time cargo run --release -- --max-memory 52428800 --chunked "$TEST_FILE" >/dev/null 2>/dev/null

# Test 2: Normal memory (should work well)
echo -e "\n${YELLOW}Test 2: Normal Memory (10MB)${NC}"
echo "Command: cargo run --release -- --max-memory 10485760 --chunked $TEST_FILE"
time cargo run --release -- --max-memory 10485760 --chunked "$TEST_FILE" >/dev/null 2>/dev/null

# Test 3: Tight memory (should show warnings)
echo -e "\n${YELLOW}Test 3: Tight Memory (2MB) - Should show memory pressure${NC}"
echo "Command: cargo run --release -- --max-memory 2097152 --chunked $TEST_FILE"
echo "Output (first 10 lines):"
timeout 10s cargo run --release -- --max-memory 2097152 --chunked "$TEST_FILE" 2>&1 | head -10 || true

# Test 4: Very tight memory (may fail or show emergency allocation)
echo -e "\n${YELLOW}Test 4: Very Tight Memory (1MB) - May trigger emergency allocation${NC}"
echo "Command: cargo run --release -- --max-memory 1048576 --chunked $TEST_FILE"
echo "Output (first 10 lines):"
timeout 10s cargo run --release -- --max-memory 1048576 --chunked "$TEST_FILE" 2>&1 | head -10 || true

# Test 5: Compare with unlimited memory
echo -e "\n${YELLOW}Test 5: No Memory Limit (Baseline)${NC}"
echo "Command: cargo run --release -- --no-chunked $TEST_FILE"
time cargo run --release -- --no-chunked "$TEST_FILE" >/dev/null 2>/dev/null

# Strategy comparison
echo -e "\n${BLUE}=== Strategy Comparison (5MB Memory Limit) ===${NC}"

strategies=("static" "adaptive" "conservative")
for strategy in "${strategies[@]}"; do
    echo -e "\n${YELLOW}Testing $strategy strategy:${NC}"
    time cargo run --release -- --max-memory 5242880 --chunk-strategy "$strategy" --chunked "$TEST_FILE" >/dev/null 2>/dev/null
done

# Cleanup
echo -e "\n${YELLOW}Cleaning up...${NC}"
rm -f "$TEST_FILE"

echo -e "\n${GREEN}Quick benchmark complete!${NC}"
echo ""
echo "Key observations:"
echo "- Generous memory should be fastest"
echo "- Tight memory may show warnings like '⚠️ Critical memory pressure'"
echo "- Very tight memory might show emergency allocation '🆘 Emergency memory allocation'"
echo "- Different strategies should show varying performance characteristics"
echo "- Memory budget adds minimal overhead compared to unlimited memory"