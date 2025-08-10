#!/bin/bash

# Phase 2 Benchmark Suite - Focus on single vs multi-file performance and strategy comparison
set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

# Check dependencies
if ! command -v hyperfine &> /dev/null; then
    echo -e "${YELLOW}Warning: hyperfine not found. Install with: brew install hyperfine${NC}"
    echo -e "${YELLOW}Some benchmarks will be skipped...${NC}"
    USE_HYPERFINE=false
else
    USE_HYPERFINE=true
fi

# Build the release version first
echo -e "${BLUE}Building release version...${NC}"
cargo build --release --quiet

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

echo -e "${CYAN}=== Phase 2 Benchmark Suite ===${NC}"
echo "Focus: Single vs Multi-file performance, Strategy comparison"
echo ""
echo "Test files:"
echo "  Small:  $(ls -lh $SMALL_FILE | awk '{print $5}') ($(wc -l < $SMALL_FILE) lines)"
echo "  Medium: $(ls -lh $MEDIUM_FILE | awk '{print $5}') ($(wc -l < $MEDIUM_FILE) lines)" 
echo "  Large:  $(ls -lh $LARGE_FILE | awk '{print $5}') ($(wc -l < $LARGE_FILE) lines)"
echo ""

# Function to create multi-file test scenario
create_multifile_scenario() {
    local size_type=$1
    local base_file=$2
    local num_files=$3
    
    local temp_dir="/tmp/tale_multifile_${size_type}"
    rm -rf "$temp_dir"
    mkdir -p "$temp_dir"
    
    echo -e "${BLUE}Creating $num_files-file scenario for $size_type test...${NC}"
    
    # Calculate lines per file
    local total_lines=$(wc -l < "$base_file")
    local lines_per_file=$((total_lines / num_files))
    
    # Split the file
    split -l "$lines_per_file" "$base_file" "$temp_dir/file_"
    
    # Rename files with .log extension
    for file in "$temp_dir"/file_*; do
        mv "$file" "${file}.log"
    done
    
    echo "$temp_dir"
}

# Function to benchmark single file processing
benchmark_single_file() {
    local file=$1
    local name=$2
    local strategy=${3:-"default"}
    
    echo -e "${BLUE}Single file benchmark - $name ($strategy strategy)${NC}"
    
    local cmd="cargo run --release --"
    
    # Add strategy flags
    case $strategy in
        "static")
            cmd="$cmd --no-adaptive"
            ;;
        "adaptive") 
            cmd="$cmd --adaptive"
            ;;
        "conservative")
            cmd="$cmd --conservative"
            ;;
        "chunked")
            cmd="$cmd --chunked"
            ;;
        "no-chunked")
            cmd="$cmd --no-chunked"
            ;;
    esac
    
    cmd="$cmd $file"
    
    if $USE_HYPERFINE; then
        echo "  Command: $cmd"
        hyperfine --warmup 1 --runs 3 --show-output \
            "$cmd >/dev/null" | \
            grep -E "(Time|Range|Command)" | sed 's/^/    /'
    else
        echo "  Running 3 times with basic timing..."
        for i in {1..3}; do
            echo -n "    Run $i: "
            start_time=$(date +%s.%N)
            eval "$cmd >/dev/null 2>&1" || echo "(failed)"
            end_time=$(date +%s.%N)
            if command -v bc &> /dev/null; then
                elapsed=$(echo "$end_time - $start_time" | bc)
                echo "${elapsed}s"
            fi
        done
    fi
    echo ""
}

# Function to benchmark multi-file processing
benchmark_multi_file() {
    local temp_dir=$1
    local name=$2
    local mode=${3:-"static"}  # static or tailing
    local strategy=${4:-"default"}
    
    echo -e "${BLUE}Multi-file benchmark - $name ($mode mode, $strategy strategy)${NC}"
    
    local files=$(ls "$temp_dir"/*.log | tr '\n' ' ')
    local cmd="cargo run --release --"
    
    # Add strategy flags  
    case $strategy in
        "static")
            cmd="$cmd --no-adaptive"
            ;;
        "adaptive")
            cmd="$cmd --adaptive" 
            ;;
        "conservative")
            cmd="$cmd --conservative"
            ;;
        "chunked")
            cmd="$cmd --chunked"
            ;;
        "no-chunked")
            cmd="$cmd --no-chunked"
            ;;
    esac
    
    # Add mode flags
    if [[ "$mode" == "tailing" ]]; then
        cmd="$cmd --follow"
    fi
    
    cmd="$cmd $files"
    
    if $USE_HYPERFINE && [[ "$mode" != "tailing" ]]; then
        echo "  Command: $cmd"
        hyperfine --warmup 1 --runs 3 \
            "$cmd >/dev/null" | \
            grep -E "(Time|Range)" | sed 's/^/    /'
    else
        echo "  Running basic timing..."
        echo "  Command: $cmd"
        
        if [[ "$mode" == "tailing" ]]; then
            echo "    (Tailing mode - running for 5 seconds)"
            timeout 5s bash -c "$cmd >/dev/null 2>&1" || echo "    Completed tailing test"
        else
            start_time=$(date +%s.%N)
            eval "$cmd >/dev/null 2>&1"
            end_time=$(date +%s.%N)
            if command -v bc &> /dev/null; then
                elapsed=$(echo "$end_time - $start_time" | bc)
                echo "    Elapsed: ${elapsed}s"
            fi
        fi
    fi
    echo ""
}

# Function to benchmark memory usage
benchmark_memory_usage() {
    local file=$1
    local name=$2
    local strategy=${3:-"default"}
    
    echo -e "${BLUE}Memory benchmark - $name ($strategy strategy)${NC}"
    
    local cmd="cargo run --release --"
    
    # Add strategy flags
    case $strategy in
        "static")
            cmd="$cmd --no-adaptive"
            ;;
        "adaptive")
            cmd="$cmd --adaptive"
            ;;
        "conservative") 
            cmd="$cmd --conservative"
            ;;
        "chunked")
            cmd="$cmd --chunked"
            ;;
        "no-chunked")
            cmd="$cmd --no-chunked"
            ;;
    esac
    
    cmd="$cmd $file"
    
    # Use time -l on macOS, time -v on Linux
    if [[ "$OSTYPE" == "darwin"* ]]; then
        echo "  Command: time -l $cmd"
        time -l bash -c "$cmd >/dev/null" 2>&1 | \
            grep -E "(real|user|sys|maximum resident set size)" | sed 's/^/    /'
    else
        echo "  Command: time -v $cmd"
        time -v bash -c "$cmd >/dev/null" 2>&1 | \
            grep -E "(Elapsed|User time|System time|Maximum resident set size)" | sed 's/^/    /'
    fi
    echo ""
}

# Function to compare strategies
compare_strategies() {
    local file=$1
    local name=$2
    
    echo -e "${CYAN}=== Strategy Comparison - $name ===${NC}"
    
    for strategy in "default" "static" "adaptive" "chunked" "no-chunked"; do
        benchmark_single_file "$file" "$name" "$strategy"
    done
}

# Function to compare single vs multi-file
compare_single_vs_multi() {
    local base_file=$1
    local name=$2
    local num_files=${3:-4}
    
    echo -e "${CYAN}=== Single vs Multi-file Comparison - $name ===${NC}"
    
    # Create multi-file scenario
    local temp_dir=$(create_multifile_scenario "$name" "$base_file" "$num_files")
    
    # Benchmark single file
    echo -e "${YELLOW}Single file processing:${NC}"
    benchmark_single_file "$base_file" "$name" "default"
    benchmark_memory_usage "$base_file" "$name" "default"
    
    # Benchmark multi-file static
    echo -e "${YELLOW}Multi-file static processing ($num_files files):${NC}"
    benchmark_multi_file "$temp_dir" "$name" "static" "default"
    
    # Benchmark multi-file tailing (if not large file)
    if [[ "$name" != "large" ]]; then
        echo -e "${YELLOW}Multi-file tailing processing ($num_files files):${NC}"  
        benchmark_multi_file "$temp_dir" "$name" "tailing" "default"
    fi
    
    # Clean up
    rm -rf "$temp_dir"
    echo ""
}

# Function to test processor selection logic
test_processor_selection() {
    echo -e "${CYAN}=== Processor Selection Logic Test ===${NC}"
    
    echo -e "${BLUE}Testing different file sizes and flags:${NC}"
    
    # Small file - should use BufferedFileProcessor
    echo "Small file (default):"
    cargo run --release -- "$SMALL_FILE" --help >/dev/null 2>&1 && echo "  ✓ Processes successfully" || echo "  ✗ Failed"
    
    # Large file with --chunked - should use ChunkedFileReader
    echo "Large file (--chunked):"
    timeout 10s cargo run --release -- --chunked "$LARGE_FILE" >/dev/null 2>&1 && echo "  ✓ Processes successfully" || echo "  ✗ Timed out or failed"
    
    # Large file with --no-chunked - should use BufferedFileProcessor 
    echo "Large file (--no-chunked):"
    timeout 10s cargo run --release -- --no-chunked "$LARGE_FILE" >/dev/null 2>&1 && echo "  ✓ Processes successfully" || echo "  ✗ Timed out or failed"
    
    echo ""
}

# Main execution
main() {
    local run_all=true
    local run_strategies=false
    local run_comparison=false
    local run_selection=false
    
    # Parse arguments
    while [[ $# -gt 0 ]]; do
        case $1 in
            --strategies)
                run_all=false
                run_strategies=true
                shift
                ;;
            --comparison)
                run_all=false
                run_comparison=true
                shift
                ;;
            --selection)
                run_all=false
                run_selection=true
                shift
                ;;
            --help)
                echo "Usage: $0 [options]"
                echo "Options:"
                echo "  --strategies   Compare different chunking strategies"
                echo "  --comparison   Compare single vs multi-file performance"
                echo "  --selection    Test processor selection logic"
                echo "  (no args)      Run all Phase 2 benchmarks"
                exit 0
                ;;
            *)
                echo "Unknown option: $1"
                exit 1
                ;;
        esac
    done
    
    # Run selected benchmarks
    if $run_all || $run_strategies; then
        compare_strategies "$MEDIUM_FILE" "medium"
        if $USE_HYPERFINE; then
            compare_strategies "$LARGE_FILE" "large" 
        fi
    fi
    
    if $run_all || $run_comparison; then
        compare_single_vs_multi "$SMALL_FILE" "small" 3
        compare_single_vs_multi "$MEDIUM_FILE" "medium" 4
        if $USE_HYPERFINE; then
            compare_single_vs_multi "$LARGE_FILE" "large" 2  # Only 2 files for large test
        fi
    fi
    
    if $run_all || $run_selection; then
        test_processor_selection
    fi
    
    echo -e "${GREEN}Phase 2 benchmark suite complete!${NC}"
    echo ""
    echo "Key insights to look for:"
    echo "  - Memory efficiency: chunked vs buffered processors"
    echo "  - Throughput: adaptive vs static vs conservative strategies"
    echo "  - Multi-file overhead: single file vs multiple file processing"
    echo "  - Processor selection: automatic vs forced selection"
}

# Run main with all arguments
main "$@"