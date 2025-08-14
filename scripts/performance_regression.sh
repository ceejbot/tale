#!/bin/bash
#
# Performance Regression Benchmark
# 
# Compare performance across different configurations to identify regressions
# and validate optimization improvements

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

echo -e "${BLUE}=== Performance Regression Benchmark ===${NC}"
echo "Comparing performance across different tale configurations"
echo ""

# Test configurations
BENCHMARK_DIR="$PROJECT_ROOT/tmp/perf_benchmark"
mkdir -p "$BENCHMARK_DIR"

# Generate comprehensive test files
generate_test_files() {
    echo -e "${YELLOW}Generating performance test files...${NC}"
    
    # Small file (1MB) - for quick processing tests
    local small_file="$BENCHMARK_DIR/small_test.log"
    {
        for i in $(seq 1 5000); do
            echo "{\"timestamp\":\"2025-08-11T10:$((i % 60)):$((i % 60))Z\",\"level\":\"INFO\",\"message\":\"Test message $i\",\"request_id\":\"req_$i\"}"
        done
    } > "$small_file"
    
    # Medium file (10MB) - for realistic processing tests
    local medium_file="$BENCHMARK_DIR/medium_test.log"
    {
        for i in $(seq 1 50000); do
            local level=$(shuf -n1 -e "INFO" "WARN" "ERROR" "DEBUG")
            if [ $((i % 3)) -eq 0 ]; then
                # Canonical format (fast path)
                echo "{\"timestamp\":\"2025-08-11T10:$((i % 60)):$((i % 60))Z\",\"level\":\"$level\",\"message\":\"Performance test message $i with some additional content\",\"method\":\"GET\",\"url\":\"/api/perf/$i\",\"status\":200,\"elapsed\":\"${i}ms\",\"size\":$((i * 5)),\"request_id\":\"perf_$i\",\"remote_host\":\"10.0.1.1\",\"user_agent\":\"PerfTestClient/1.0\"}"
            else
                # Message format (flexible path)
                echo "{\"timestamp\":\"2025-08-11T10:$((i % 60)):$((i % 60))Z\",\"level\":\"$level\",\"message\":\"Flexible test message $i\",\"request_id\":\"msg_$i\",\"module\":\"perf_test\",\"file\":\"test.rs\",\"line\":$((i % 1000))}"
            fi
        done
    } > "$medium_file"
    
    # Large file (50MB) - for stress testing
    local large_file="$BENCHMARK_DIR/large_test.log"
    {
        for i in $(seq 1 250000); do
            local level=$(shuf -n1 -e "INFO" "WARN" "ERROR" "DEBUG")
            echo "{\"timestamp\":\"2025-08-11T$((10 + i % 12)):$((i % 60)):$((i % 60))Z\",\"level\":\"$level\",\"message\":\"Large file test message $i with extended content for realistic log sizes\",\"method\":\"POST\",\"url\":\"/api/large/endpoint/$i\",\"status\":$((200 + i % 300)),\"elapsed\":\"$((i % 5000))ms\",\"size\":$((i % 100000)),\"request_id\":\"large_$i\",\"remote_host\":\"192.168.$((i % 255)).$((i % 255))\",\"user_agent\":\"LargeTestClient/2.0\",\"extra_field_$((i % 10))\":\"extra_value_$i\"}"
        done
    } > "$large_file"
    
    echo -e "${GREEN}Generated test files:${NC}"
    echo "  Small:  $(du -h "$small_file" | cut -f1) ($(wc -l < "$small_file") lines)"
    echo "  Medium: $(du -h "$medium_file" | cut -f1) ($(wc -l < "$medium_file") lines)"
    echo "  Large:  $(du -h "$large_file" | cut -f1) ($(wc -l < "$large_file") lines)"
    echo ""
}

# Run a timed benchmark
run_timed_test() {
    local test_name=$1
    local command=$2
    local iterations=${3:-3}
    
    echo -e "${YELLOW}--- $test_name ---${NC}"
    echo "Command: $command"
    echo "Iterations: $iterations"
    
    local total_time=0
    local times=()
    
    for i in $(seq 1 $iterations); do
        echo -n "  Run $i/$iterations... "
        
        local start=$(date +%s.%N)
        eval "$command" >/dev/null 2>&1
        local end=$(date +%s.%N)
        
        local duration=$(echo "$end - $start" | bc -l)
        times+=($duration)
        total_time=$(echo "$total_time + $duration" | bc -l)
        
        printf "%.3fs\n" "$duration"
    done
    
    local avg_time=$(echo "scale=3; $total_time / $iterations" | bc -l)
    
    # Calculate min and max
    local min_time=${times[0]}
    local max_time=${times[0]}
    for time in "${times[@]}"; do
        if (( $(echo "$time < $min_time" | bc -l) )); then
            min_time=$time
        fi
        if (( $(echo "$time > $max_time" | bc -l) )); then
            max_time=$time
        fi
    done
    
    printf "${GREEN}  Average: %.3fs, Min: %.3fs, Max: %.3fs${NC}\n" "$avg_time" "$min_time" "$max_time"
    echo ""
}

# Test different processing modes
test_processing_modes() {
    local test_file=$1
    local file_desc=$2
    
    echo -e "${BLUE}=== Processing Mode Comparison ($file_desc) ===${NC}"
    
    # Standard processing (no chunking)
    run_timed_test \
        "Standard Processing" \
        "cargo run --release -- --no-chunked $test_file"
    
    # Chunked processing with default settings
    run_timed_test \
        "Chunked Processing (Default)" \
        "cargo run --release -- --chunked $test_file"
    
    # Chunked with memory budget (normal)
    run_timed_test \
        "Chunked + Memory Budget (100MB)" \
        "cargo run --release -- --chunked --max-memory 104857600 $test_file"
    
    # Chunked with tight memory budget
    run_timed_test \
        "Chunked + Memory Budget (10MB)" \
        "cargo run --release -- --chunked --max-memory 10485760 $test_file"
}

# Test chunking strategies
test_chunking_strategies() {
    local test_file=$1
    local file_desc=$2
    
    echo -e "${BLUE}=== Chunking Strategy Comparison ($file_desc) ===${NC}"
    
    local strategies=("static" "adaptive" "conservative")
    
    for strategy in "${strategies[@]}"; do
        run_timed_test \
            "Strategy: $strategy" \
            "cargo run --release -- --chunked --chunk-strategy $strategy $test_file"
    done
}

# Test memory pressure scenarios
test_memory_pressure() {
    local test_file=$1
    local file_desc=$2
    
    echo -e "${BLUE}=== Memory Pressure Impact ($file_desc) ===${NC}"
    
    # Different memory limits to test pressure levels
    local limits=("50MB" "20MB" "10MB" "5MB" "2MB")
    
    for limit in "${limits[@]}"; do
        local limit_bytes
        case $limit in
            *MB) limit_bytes=$((${limit%MB} * 1024 * 1024)) ;;
            *) limit_bytes=$limit ;;
        esac
        
        run_timed_test \
            "Memory Limit: $limit" \
            "cargo run --release -- --chunked --max-memory $limit_bytes $test_file"
    done
}

# Test JSON profiling impact
test_profiling_overhead() {
    local test_file=$1
    local file_desc=$2
    
    echo -e "${BLUE}=== JSON Profiling Overhead ($file_desc) ===${NC}"
    
    # Only available in debug builds
    if cargo build 2>&1 | grep -q "Finished"; then
        echo -e "${YELLOW}Testing debug build with profiling...${NC}"
        
        run_timed_test \
            "Debug Build (with profiling)" \
            "cargo run -- --profile-json $test_file"
        
        run_timed_test \
            "Debug Build (no profiling)" \
            "cargo run -- $test_file"
    else
        echo -e "${YELLOW}Skipping profiling overhead test (debug build failed)${NC}"
    fi
    
    echo -e "${YELLOW}Release build baseline:${NC}"
    run_timed_test \
        "Release Build" \
        "cargo run --release -- $test_file"
}

# Multi-file performance test
test_multi_file() {
    echo -e "${BLUE}=== Multi-File Processing Performance ===${NC}"
    
    # Create multiple small files
    local multi_dir="$BENCHMARK_DIR/multi_files"
    mkdir -p "$multi_dir"
    
    echo -e "${YELLOW}Creating multiple test files...${NC}"
    for i in $(seq 1 5); do
        {
            for j in $(seq 1 10000); do
                echo "{\"timestamp\":\"2025-08-11T10:00:00Z\",\"level\":\"INFO\",\"message\":\"Multi-file test $i:$j\",\"request_id\":\"multi_${i}_${j}\"}"
            done
        } > "$multi_dir/file_$i.log"
    done
    
    echo "Created 5 files with 10k lines each"
    echo ""
    
    # Test multi-file processing
    run_timed_test \
        "Multi-File (5 files)" \
        "cargo run --release -- $multi_dir/*.log"
    
    run_timed_test \
        "Multi-File + Memory Budget" \
        "cargo run --release -- --max-memory 20971520 $multi_dir/*.log"
    
    run_timed_test \
        "Multi-File + Chunked" \
        "cargo run --release -- --chunked $multi_dir/*.log"
}

# Throughput analysis
analyze_throughput() {
    local test_file=$1
    local file_desc=$2
    
    echo -e "${BLUE}=== Throughput Analysis ($file_desc) ===${NC}"
    
    local file_size=$(stat -f%z "$test_file" 2>/dev/null || stat -c%s "$test_file" 2>/dev/null || echo "0")
    local line_count=$(wc -l < "$test_file")
    
    echo "File size: $(echo "scale=2; $file_size / 1024 / 1024" | bc -l) MB"
    echo "Line count: $line_count"
    echo ""
    
    # Test different configurations and calculate throughput
    local configs=(
        "Standard|cargo run --release -- --no-chunked $test_file"
        "Chunked|cargo run --release -- --chunked $test_file"  
        "Memory Budget|cargo run --release -- --chunked --max-memory 10485760 $test_file"
    )
    
    for config in "${configs[@]}"; do
        local name="${config%%|*}"
        local command="${config##*|}"
        
        echo -e "${YELLOW}--- Throughput Test: $name ---${NC}"
        
        local start=$(date +%s.%N)
        eval "$command" >/dev/null 2>&1
        local end=$(date +%s.%N)
        
        local duration=$(echo "$end - $start" | bc -l)
        local mb_per_sec=$(echo "scale=2; ($file_size / 1024 / 1024) / $duration" | bc -l)
        local lines_per_sec=$(echo "scale=0; $line_count / $duration" | bc -l)
        
        printf "  Duration: %.3fs\n" "$duration"
        printf "  Throughput: %.2f MB/s\n" "$mb_per_sec"
        printf "  Lines/sec: %s\n" "$lines_per_sec"
        echo ""
    done
}

# Main execution
main() {
    echo "Starting performance regression benchmark..."
    echo ""
    
    # Build release binary
    echo -e "${YELLOW}Building release binary...${NC}"
    cargo build --release
    echo ""
    
    # Generate test files
    generate_test_files
    
    # Run comprehensive tests
    test_processing_modes "$BENCHMARK_DIR/small_test.log" "Small File 1MB"
    test_processing_modes "$BENCHMARK_DIR/medium_test.log" "Medium File 10MB"
    
    test_chunking_strategies "$BENCHMARK_DIR/medium_test.log" "Medium File"
    
    test_memory_pressure "$BENCHMARK_DIR/medium_test.log" "Medium File"
    
    test_profiling_overhead "$BENCHMARK_DIR/small_test.log" "Small File"
    
    test_multi_file
    
    analyze_throughput "$BENCHMARK_DIR/medium_test.log" "Medium File"
    analyze_throughput "$BENCHMARK_DIR/large_test.log" "Large File"
    
    # Cleanup
    echo -e "${YELLOW}Cleaning up...${NC}"
    rm -rf "$BENCHMARK_DIR"
    
    echo -e "${GREEN}Performance regression benchmark complete!${NC}"
    echo ""
    echo "Key metrics to analyze:"
    echo "- Processing time differences between configurations"
    echo "- Memory budget impact on performance"
    echo "- Chunking strategy effectiveness"
    echo "- Throughput (MB/s and lines/sec) under different conditions"
    echo "- Multi-file processing overhead"
}

# Help function
if [[ "${1:-}" == "--help" || "${1:-}" == "-h" ]]; then
    echo "Performance Regression Benchmark"
    echo ""
    echo "This script comprehensively tests tale performance across:"
    echo "  - Different processing modes (chunked vs standard)"
    echo "  - Various chunking strategies (static, adaptive, conservative)"  
    echo "  - Memory pressure scenarios (different limits)"
    echo "  - Multi-file processing"
    echo "  - Throughput analysis (MB/s and lines/sec)"
    echo ""
    echo "Usage: $0"
    echo ""
    echo "Output includes timing, throughput, and performance comparisons."
    exit 0
fi

main