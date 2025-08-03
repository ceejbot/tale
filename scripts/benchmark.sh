#!/bin/bash

# Benchmark script for tale - measures performance and memory usage
set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Check if we have hyperfine installed
if ! command -v hyperfine &> /dev/null; then
    echo -e "${YELLOW}Warning: hyperfine not found. Install with: brew install hyperfine${NC}"
    echo -e "${YELLOW}Falling back to basic timing...${NC}"
    USE_HYPERFINE=false
else
    USE_HYPERFINE=true
fi

# Build the release version first
echo -e "${BLUE}Building release version...${NC}"
cargo build --release

# Test files
SMALL_FILE="fixtures/benchmarks/small.log"
MEDIUM_FILE="fixtures/benchmarks/medium.log" 
LARGE_FILE="fixtures/benchmarks/large.log"

# Check if test files exist
for file in "$SMALL_FILE" "$MEDIUM_FILE" "$LARGE_FILE"; do
    if [[ ! -f "$file" ]]; then
        echo -e "${RED}Error: $file not found. Run scripts/generate_test_data.py first.${NC}"
        exit 1
    fi
done

echo -e "${GREEN}=== Tale Benchmark Suite ===${NC}"
echo "Test files:"
echo "  Small:  $(ls -lh $SMALL_FILE | awk '{print $5}') (1K lines)"
echo "  Medium: $(ls -lh $MEDIUM_FILE | awk '{print $5}') (100K lines)" 
echo "  Large:  $(ls -lh $LARGE_FILE | awk '{print $5}') (1M lines)"
echo ""

# Function to run memory benchmark
run_memory_benchmark() {
    local file=$1
    local name=$2
    
    echo -e "${BLUE}Memory benchmark - $name${NC}"
    
    # Use time -l on macOS, time -v on Linux
    if [[ "$OSTYPE" == "darwin"* ]]; then
        TIME_CMD="time -l"
        MEMORY_FIELD="\$10"  # Maximum resident set size
    else
        TIME_CMD="time -v"
        MEMORY_FIELD="grep 'Maximum resident set size' | awk '{print \$6}'"
    fi
    
    # Run the benchmark
    echo "  Running: cargo run --release -- $file >/dev/null"
    eval "$TIME_CMD cargo run --release -- $file >/dev/null" 2>&1 | \
        if [[ "$OSTYPE" == "darwin"* ]]; then
            grep -E "(real|user|sys|maximum resident set size)" | sed 's/^/    /'
        else
            grep -E "(Elapsed|User time|System time|Maximum resident set size)" | sed 's/^/    /'
        fi
    echo ""
}

# Function to run throughput benchmark
run_throughput_benchmark() {
    local file=$1
    local name=$2
    
    echo -e "${BLUE}Throughput benchmark - $name${NC}"
    
    if $USE_HYPERFINE; then
        echo "  Running hyperfine..."
        hyperfine --warmup 1 --runs 3 --export-json /tmp/benchmark_${name}.json \
            "cargo run --release -- $file >/dev/null" | \
            grep -E "(Time|Range)" | sed 's/^/    /'
    else
        echo "  Running basic timing (3 runs)..."
        for i in {1..3}; do
            echo -n "    Run $i: "
            time (cargo run --release -- "$file" >/dev/null) 2>&1 | \
                grep real | awk '{print $2}'
        done
    fi
    echo ""
}

# Function to run stdin benchmark
run_stdin_benchmark() {
    local file=$1
    local name=$2
    
    echo -e "${BLUE}Stdin benchmark - $name${NC}"
    
    if $USE_HYPERFINE; then
        echo "  Running hyperfine with stdin..."
        hyperfine --warmup 1 --runs 3 \
            "cat $file | cargo run --release >/dev/null" | \
            grep -E "(Time|Range)" | sed 's/^/    /'
    else
        echo "  Running basic timing (3 runs) with stdin..."
        for i in {1..3}; do
            echo -n "    Run $i: "
            time (cat "$file" | cargo run --release >/dev/null) 2>&1 | \
                grep real | awk '{print $2}'
        done
    fi
    echo ""
}

# Function to calculate processing rates
calculate_rates() {
    local file=$1
    local name=$2
    
    echo -e "${BLUE}Processing rates - $name${NC}"
    
    local lines=$(wc -l < "$file")
    local size_mb=$(du -m "$file" | cut -f1)
    
    echo "  File info: $lines lines, ${size_mb}MB"
    
    # Quick single run to get baseline timing
    local start_time=$(date +%s.%N)
    cargo run --release -- "$file" >/dev/null 2>&1
    local end_time=$(date +%s.%N)
    local elapsed=$(echo "$end_time - $start_time" | bc)
    
    if command -v bc &> /dev/null; then
        local lines_per_sec=$(echo "scale=0; $lines / $elapsed" | bc)
        local mb_per_sec=$(echo "scale=2; $size_mb / $elapsed" | bc)
        echo "  Processing rate: $lines_per_sec lines/sec, ${mb_per_sec} MB/sec"
    else
        echo "  Elapsed time: ${elapsed}s"
    fi
    echo ""
}

# Function to test output correctness
test_correctness() {
    echo -e "${BLUE}Output correctness test${NC}"
    
    # Test with a small sample to ensure output is identical
    local test_lines=100
    head -n $test_lines "$SMALL_FILE" > /tmp/test_sample.log
    
    # Generate reference output
    cargo run --release -- /tmp/test_sample.log > /tmp/reference_output.txt 2>/dev/null
    
    # Test file input vs stdin input
    cat /tmp/test_sample.log | cargo run --release > /tmp/stdin_output.txt 2>/dev/null
    
    if diff /tmp/reference_output.txt /tmp/stdin_output.txt >/dev/null; then
        echo -e "  ${GREEN}✓ File and stdin output identical${NC}"
    else
        echo -e "  ${RED}✗ File and stdin output differ${NC}"
    fi
    
    # Clean up temp files
    rm -f /tmp/test_sample.log /tmp/reference_output.txt /tmp/stdin_output.txt
    echo ""
}

# Main benchmark execution
main() {
    local run_all=true
    local run_memory=false
    local run_throughput=false
    local run_stdin=false
    local run_rates=false
    
    # Parse command line arguments
    while [[ $# -gt 0 ]]; do
        case $1 in
            --memory)
                run_all=false
                run_memory=true
                shift
                ;;
            --throughput)
                run_all=false  
                run_throughput=true
                shift
                ;;
            --stdin)
                run_all=false
                run_stdin=true
                shift
                ;;
            --rates)
                run_all=false
                run_rates=true
                shift
                ;;
            --help)
                echo "Usage: $0 [--memory] [--throughput] [--stdin] [--rates]"
                echo "  --memory     Run only memory benchmarks"
                echo "  --throughput Run only throughput benchmarks"
                echo "  --stdin      Run only stdin benchmarks"
                echo "  --rates      Run only processing rate calculations"
                echo "  (no args)    Run all benchmarks"
                exit 0
                ;;
            *)
                echo "Unknown option: $1"
                exit 1
                ;;
        esac
    done
    
    # Run correctness test first
    test_correctness
    
    # Run selected benchmarks
    for file_info in "small:$SMALL_FILE" "medium:$MEDIUM_FILE" "large:$LARGE_FILE"; do
        IFS=':' read -r name file <<< "$file_info"
        
        if $run_all || $run_memory; then
            run_memory_benchmark "$file" "$name"
        fi
        
        if $run_all || $run_throughput; then
            run_throughput_benchmark "$file" "$name"
        fi
        
        if $run_all || $run_stdin; then
            run_stdin_benchmark "$file" "$name"
        fi
        
        if $run_all || $run_rates; then
            calculate_rates "$file" "$name"
        fi
    done
    
    echo -e "${GREEN}Benchmark complete!${NC}"
    
    if $USE_HYPERFINE && ($run_all || $run_throughput); then
        echo ""
        echo "Detailed JSON results saved to:"
        for name in small medium large; do
            if [[ -f "/tmp/benchmark_${name}.json" ]]; then
                echo "  /tmp/benchmark_${name}.json"
            fi
        done
    fi
}

# Run the main function with all arguments
main "$@"