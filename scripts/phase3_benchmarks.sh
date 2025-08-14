#!/bin/bash
#
# Phase 3 Multi-file Benchmarks: Memory Budget & Performance Testing
# 
# This script tests various scenarios to validate memory budget effectiveness,
# multi-file processing performance, and adaptive chunking strategies under
# different memory constraints and file configurations.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "$PROJECT_ROOT"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Test configuration
BENCHMARK_DIR="$PROJECT_ROOT/tmp/benchmarks"
SMALL_FILE_SIZE="1MB"    # 1MB files
MEDIUM_FILE_SIZE="10MB"  # 10MB files  
LARGE_FILE_SIZE="50MB"   # 50MB files
NUM_FILES_SMALL=10       # Many small files
NUM_FILES_MEDIUM=5       # Some medium files
NUM_FILES_LARGE=3        # Few large files

# Memory limits for testing (in bytes)
MEMORY_LIMIT_TIGHT="10MB"    # 10MB - very constrained
MEMORY_LIMIT_NORMAL="100MB"  # 100MB - reasonable
MEMORY_LIMIT_GENEROUS="500MB" # 500MB - generous

echo -e "${BLUE}=== Phase 3 Multi-File Benchmarks ===${NC}"
echo "Testing memory budget management and multi-file performance"
echo ""

# Clean up function
cleanup() {
    echo -e "${YELLOW}Cleaning up benchmark files...${NC}"
    rm -rf "$BENCHMARK_DIR" 2>/dev/null || true
}
trap cleanup EXIT

# Create benchmark directory
mkdir -p "$BENCHMARK_DIR"

# Helper function to convert human sizes to bytes
to_bytes() {
    local size=$1
    case $size in
        *KB) echo $((${size%KB} * 1024)) ;;
        *MB) echo $((${size%MB} * 1024 * 1024)) ;;
        *GB) echo $((${size%GB} * 1024 * 1024 * 1024)) ;;
        *) echo "$size" ;;
    esac
}

# Generate test data files
generate_test_files() {
    local file_type=$1
    local file_size=$2
    local num_files=$3
    local dir_suffix=$4
    
    local test_dir="$BENCHMARK_DIR/${file_type}_files_${dir_suffix}"
    mkdir -p "$test_dir"
    
    echo -e "${BLUE}Generating $num_files x $file_size $file_type files...${NC}"
    
    local size_bytes=$(to_bytes "$file_size")
    local lines_per_file=$((size_bytes / 200))  # Assume ~200 bytes per JSON line
    
    for i in $(seq 1 $num_files); do
        local filename="$test_dir/test_${file_type}_${i}.log"
        
        # Generate realistic JSON log lines
        {
            for j in $(seq 1 $lines_per_file); do
                local timestamp=$(date -u +"%Y-%m-%dT%H:%M:%S.%3NZ")
                local level=$(shuf -n1 -e "INFO" "WARN" "ERROR" "DEBUG")
                local message="Processing request batch $j in file $i"
                local method=$(shuf -n1 -e "GET" "POST" "PUT" "DELETE")
                local status=$(shuf -n1 -e "200" "404" "500" "201" "400")
                local elapsed=$((RANDOM % 5000 + 1))
                local size=$((RANDOM % 10000 + 100))
                
                # Mix of canonical and message formats for realistic parsing
                if [ $((j % 3)) -eq 0 ]; then
                    # Canonical format (fast path)
                    echo "{\"timestamp\":\"$timestamp\",\"level\":\"$level\",\"message\":\"$message\",\"method\":\"$method\",\"url\":\"/api/v1/data/$j\",\"status\":$status,\"elapsed\":\"${elapsed}ms\",\"size\":$size,\"request_id\":\"req_${i}_${j}\",\"remote_host\":\"10.0.1.$((RANDOM % 255))\",\"user_agent\":\"TestClient/1.0\"}"
                else
                    # Message format (flexible path)
                    echo "{\"timestamp\":\"$timestamp\",\"level\":\"$level\",\"message\":\"$message\",\"request_id\":\"req_${i}_${j}\",\"duration\":$elapsed}"
                fi
            done
        } > "$filename"
        
        if [ $((i % 5)) -eq 0 ]; then
            echo "  Generated $i/$num_files files..."
        fi
    done
    
    echo -e "${GREEN}Generated $num_files files in $test_dir${NC}"
}

# Run benchmark with specific parameters
run_benchmark() {
    local test_name=$1
    local test_dir=$2  
    local memory_limit=$3
    local extra_args=$4
    local description=$5
    
    echo -e "${YELLOW}--- $test_name ---${NC}"
    echo "Description: $description"
    echo "Memory limit: $memory_limit"
    echo "Extra args: $extra_args"
    echo "Files: $(ls "$test_dir"/*.log | wc -l)"
    echo ""
    
    local memory_bytes=$(to_bytes "$memory_limit")
    local start_time=$(date +%s.%N)
    
    # Build command
    local cmd="cargo run --release -- --max-memory $memory_bytes $extra_args $test_dir/*.log"
    
    echo "Command: $cmd"
    echo ""
    
    # Run benchmark and capture output
    local temp_output=$(mktemp)
    if eval "$cmd > $temp_output 2>&1"; then
        local end_time=$(date +%s.%N)
        local duration=$(echo "$end_time - $start_time" | bc -l)
        local lines_processed=$(wc -l < "$temp_output")
        
        echo -e "${GREEN}✅ SUCCESS${NC}"
        echo "Duration: ${duration}s"
        echo "Lines processed: $lines_processed"
        echo "Lines/second: $(echo "scale=2; $lines_processed / $duration" | bc -l)"
        
        # Extract memory info from stderr if available
        if grep -q "Memory Budget Report" "$temp_output"; then
            echo ""
            echo "Memory Budget Report:"
            grep -A 10 "Memory Budget Report" "$temp_output" | tail -10
        fi
        
        # Check for JSON profiling info
        if grep -q "JSON Parsing Profile" "$temp_output"; then
            echo ""
            echo "JSON Parsing Profile:"
            grep -A 15 "JSON Parsing Profile" "$temp_output" | tail -15
        fi
    else
        local end_time=$(date +%s.%N)
        local duration=$(echo "$end_time - $start_time" | bc -l)
        
        echo -e "${RED}❌ FAILED${NC}"
        echo "Duration: ${duration}s"
        echo "Error output:"
        cat "$temp_output"
    fi
    
    rm -f "$temp_output"
    echo ""
    echo "----------------------------------------"
    echo ""
}

# Memory pressure test
test_memory_pressure() {
    local test_dir=$1
    local file_type=$2
    
    echo -e "${BLUE}=== Memory Pressure Tests ($file_type files) ===${NC}"
    echo ""
    
    # Test 1: Generous memory - should run smoothly
    run_benchmark \
        "Generous Memory Test" \
        "$test_dir" \
        "$MEMORY_LIMIT_GENEROUS" \
        "--chunked" \
        "Test with generous memory limit - should show low pressure"
    
    # Test 2: Normal memory - should adapt chunk sizes
    run_benchmark \
        "Normal Memory Test" \
        "$test_dir" \
        "$MEMORY_LIMIT_NORMAL" \
        "--chunked" \
        "Test with normal memory limit - should show moderate pressure adaptation"
    
    # Test 3: Tight memory - should trigger emergency measures
    run_benchmark \
        "Tight Memory Test" \
        "$test_dir" \
        "$MEMORY_LIMIT_TIGHT" \
        "--chunked" \
        "Test with tight memory limit - should show high/critical pressure"
}

# Strategy comparison test  
test_strategies() {
    local test_dir=$1
    local file_type=$2
    local memory_limit=$3
    
    echo -e "${BLUE}=== Strategy Comparison Tests ($file_type files, $memory_limit memory) ===${NC}"
    echo ""
    
    # Test different chunking strategies
    local strategies=("static" "adaptive" "conservative")
    
    for strategy in "${strategies[@]}"; do
        run_benchmark \
            "Strategy: $strategy" \
            "$test_dir" \
            "$memory_limit" \
            "--chunk-strategy $strategy --chunked" \
            "Test $strategy strategy with $memory_limit memory limit"
    done
    
    # Test without chunking for comparison
    run_benchmark \
        "No Chunking (Baseline)" \
        "$test_dir" \
        "$memory_limit" \
        "--no-chunked" \
        "Baseline test without chunked processing"
}

# Multi-file scaling test
test_multi_file_scaling() {
    echo -e "${BLUE}=== Multi-File Scaling Tests ===${NC}"
    echo ""
    
    # Test with different numbers of files
    local base_dir="$BENCHMARK_DIR/scaling_test"
    mkdir -p "$base_dir"
    
    # Generate different file count scenarios
    for file_count in 5 10 20; do
        local scaling_dir="$base_dir/${file_count}_files"
        mkdir -p "$scaling_dir"
        
        echo -e "${BLUE}Generating $file_count medium files for scaling test...${NC}"
        
        local size_bytes=$(to_bytes "5MB")  # 5MB each for scaling test
        local lines_per_file=$((size_bytes / 200))
        
        for i in $(seq 1 $file_count); do
            local filename="$scaling_dir/scale_test_${i}.log"
            
            {
                for j in $(seq 1 $lines_per_file); do
                    local timestamp=$(date -u +"%Y-%m-%dT%H:%M:%S.%3NZ")
                    echo "{\"timestamp\":\"$timestamp\",\"level\":\"INFO\",\"message\":\"Scaling test file $i line $j\",\"request_id\":\"scale_${i}_${j}\"}"
                done
            } > "$filename"
        done
        
        run_benchmark \
            "Scaling Test: $file_count files" \
            "$scaling_dir" \
            "$MEMORY_LIMIT_NORMAL" \
            "--chunked" \
            "Test processing $file_count files simultaneously"
    done
}

# Memory efficiency comparison
test_memory_efficiency() {
    local test_dir=$1
    local file_type=$2
    
    echo -e "${BLUE}=== Memory Efficiency Comparison ($file_type files) ===${NC}"
    echo ""
    
    # Test with and without memory budget
    run_benchmark \
        "With Memory Budget" \
        "$test_dir" \
        "$MEMORY_LIMIT_NORMAL" \
        "--chunked" \
        "Test with memory budget management enabled"
        
    run_benchmark \
        "No Memory Limit" \
        "$test_dir" \
        "1GB" \
        "--chunked" \
        "Test with very high memory limit (minimal budget pressure)"
}

# Main benchmark execution
main() {
    echo -e "${BLUE}Starting Phase 3 Multi-File Benchmarks...${NC}"
    echo ""
    
    # Check if cargo build works
    echo -e "${YELLOW}Building release binary...${NC}"
    cargo build --release
    echo -e "${GREEN}Build completed${NC}"
    echo ""
    
    # Generate test files
    echo -e "${BLUE}=== Generating Test Files ===${NC}"
    generate_test_files "small" "$SMALL_FILE_SIZE" "$NUM_FILES_SMALL" "memory_test"
    generate_test_files "medium" "$MEDIUM_FILE_SIZE" "$NUM_FILES_MEDIUM" "memory_test"  
    generate_test_files "large" "$LARGE_FILE_SIZE" "$NUM_FILES_LARGE" "memory_test"
    echo ""
    
    # Test 1: Memory pressure tests with different file sizes
    test_memory_pressure "$BENCHMARK_DIR/small_files_memory_test" "small"
    test_memory_pressure "$BENCHMARK_DIR/medium_files_memory_test" "medium"
    test_memory_pressure "$BENCHMARK_DIR/large_files_memory_test" "large"
    
    # Test 2: Strategy comparison
    test_strategies "$BENCHMARK_DIR/medium_files_memory_test" "medium" "$MEMORY_LIMIT_NORMAL"
    
    # Test 3: Multi-file scaling
    test_multi_file_scaling
    
    # Test 4: Memory efficiency comparison
    test_memory_efficiency "$BENCHMARK_DIR/medium_files_memory_test" "medium"
    
    echo -e "${GREEN}=== Phase 3 Benchmarks Complete ===${NC}"
    echo ""
    echo -e "${BLUE}Summary:${NC}"
    echo "- Tested memory pressure adaptation across small/medium/large files"
    echo "- Compared chunking strategies under memory constraints"  
    echo "- Validated multi-file scaling performance"
    echo "- Measured memory efficiency improvements"
    echo ""
    echo "Check the output above for performance metrics and memory usage patterns."
}

# Help function
show_help() {
    echo "Phase 3 Multi-File Benchmarks"
    echo ""
    echo "Usage: $0 [options]"
    echo ""
    echo "Options:"
    echo "  --help    Show this help message"
    echo "  --quick   Run quick tests only (fewer files)"
    echo "  --clean   Clean up benchmark files and exit"
    echo ""
    echo "This script tests:"
    echo "  - Memory budget management effectiveness"
    echo "  - Multi-file processing performance"
    echo "  - Chunking strategy comparisons"
    echo "  - Memory pressure adaptation"
    echo "  - File scaling behavior"
}

# Handle command line arguments
case "${1:-}" in
    --help|-h)
        show_help
        exit 0
        ;;
    --clean)
        cleanup
        echo "Benchmark files cleaned up"
        exit 0
        ;;
    --quick)
        # Reduce test sizes for quick testing
        NUM_FILES_SMALL=3
        NUM_FILES_MEDIUM=2
        NUM_FILES_LARGE=1
        SMALL_FILE_SIZE="500KB"
        MEDIUM_FILE_SIZE="2MB"
        LARGE_FILE_SIZE="5MB"
        echo -e "${YELLOW}Running quick benchmarks with reduced file sizes${NC}"
        ;;
esac

# Run main function
main