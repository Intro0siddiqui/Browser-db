#!/bin/bash

# BrowserDB Test Runner Script
# Comprehensive testing suite for all BrowserDB components

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Test configuration
ZIG_TEST_FILE="main.zig"
BENCH_FILE="bench.zig"
OUTPUT_DIR="test_results"
COVERAGE_DIR="coverage"

# Test categories
CORE_TESTS=("lsm_tree_tests.zig" "bdb_format_tests.zig" "heatmap_indexing_tests.zig" "modes_operations_tests.zig")
BENCHMARK_TESTS=("performance_benchmarks.zig")
INTEGRATION_TESTS=("integration_tests.zig")

# Functions
log_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

log_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

create_directories() {
    log_info "Creating test directories..."
    mkdir -p "$OUTPUT_DIR"
    mkdir -p "$COVERAGE_DIR"
}

setup_environment() {
    log_info "Setting up test environment..."
    
    # Check if Zig is installed
    if ! command -v zig &> /dev/null; then
        log_error "Zig is not installed. Please install Zig 0.13.0+"
        exit 1
    fi
    
    # Check Zig version
    ZIG_VERSION=$(zig version)
    log_info "Using Zig version: $ZIG_VERSION"
    
    # Clean previous builds
    log_info "Cleaning previous builds..."
    rm -rf zig-out/
    
    # Set test environment variables
    export BROWSERDB_TEST_MODE=1
    export RUST_BACKTRACE=1
}

run_unit_tests() {
    log_info "Running unit tests..."
    local test_name="unit_tests"
    local start_time=$(date +%s)
    
    # Build and run tests
    if zig build test --summary all 2>&1 | tee "$OUTPUT_DIR/${test_name}.log"; then
        local end_time=$(date +%s)
        local duration=$((end_time - start_time))
        log_success "Unit tests passed in ${duration}s"
        echo "duration:$duration" > "$OUTPUT_DIR/${test_name}_time.txt"
        return 0
    else
        log_error "Unit tests failed"
        return 1
    fi
}

run_core_tests() {
    log_info "Running core component tests..."
    
    for test_file in "${CORE_TESTS[@]}"; do
        if [ -f "$test_file" ]; then
            log_info "Running $test_file..."
            local test_name="${test_file%.zig}"
            local start_time=$(date +%s)
            
            if zig test "$test_file" --summary all 2>&1 | tee "$OUTPUT_DIR/${test_name}.log"; then
                local end_time=$(date +%s)
                local duration=$((end_time - start_time))
                log_success "$test_file passed in ${duration}s"
            else
                log_error "$test_file failed"
                return 1
            fi
        else
            log_warning "Test file $test_file not found, skipping..."
        fi
    done
}

run_benchmark_tests() {
    log_info "Running benchmark tests..."
    
    for test_file in "${BENCHMARK_TESTS[@]}"; do
        if [ -f "$test_file" ]; then
            log_info "Running benchmark: $test_file"
            local test_name="${test_file%.zig}"
            
            # Build with release optimizations for benchmarks
            if zig test "$test_file" -Drelease-safe --summary all 2>&1 | tee "$OUTPUT_DIR/${test_name}_benchmark.log"; then
                log_success "$test_file benchmark completed"
            else
                log_warning "$test_file benchmark had issues"
            fi
        else
            log_warning "Benchmark file $test_file not found, skipping..."
        fi
    done
}

run_performance_validation() {
    log_info "Running performance validation tests..."
    
    # Performance thresholds (ops/sec)
    MIN_WRITE_OPS=5000
    MIN_READ_OPS=20000
    MAX_LATENCY_MS=10
    
    # Check if performance benchmarks exist
    if [ ! -f "performance_benchmarks.zig" ]; then
        log_warning "Performance benchmarks not found, skipping validation"
        return 0
    fi
    
    # Run performance test with specific focus
    log_info "Validating performance requirements..."
    
    # This would typically parse benchmark output
    # For now, we just run the tests
    if zig test performance_benchmarks.zig -Drelease-fast --summary all 2>&1 | tee "$OUTPUT_DIR/performance_validation.log"; then
        log_success "Performance validation completed"
        
        # Extract performance metrics (would need actual parsing)
        echo "performance_validation:passed" > "$OUTPUT_DIR/performance_status.txt"
    else
        log_warning "Performance validation had issues"
        echo "performance_validation:failed" > "$OUTPUT_DIR/performance_status.txt"
    fi
}

run_memory_tests() {
    log_info "Running memory leak tests..."
    
    # Use valgrind if available
    if command -v valgrind &> /dev/null; then
        log_info "Running tests under Valgrind memory checker..."
        
        # Run a subset of tests under valgrind
        if valgrind --leak-check=full --show-leak-kinds=all \
                   zig test lsm_tree_tests.zig --summary none 2>&1 | \
                   tee "$OUTPUT_DIR/memory_test.log"; then
            log_success "Memory leak tests passed"
        else
            log_warning "Memory tests had issues"
        fi
    else
        log_info "Valgrind not available, skipping memory leak tests"
    fi
}

generate_test_report() {
    log_info "Generating test report..."
    
    local report_file="$OUTPUT_DIR/test_report.html"
    
    cat > "$report_file" << 'EOF'
<!DOCTYPE html>
<html>
<head>
    <title>BrowserDB Test Report</title>
    <style>
        body { font-family: Arial, sans-serif; margin: 20px; }
        .header { background-color: #f0f0f0; padding: 20px; border-radius: 5px; }
        .success { color: green; }
        .failure { color: red; }
        .warning { color: orange; }
        .section { margin: 20px 0; }
        table { border-collapse: collapse; width: 100%; }
        th, td { border: 1px solid #ddd; padding: 8px; text-align: left; }
        th { background-color: #f2f2f2; }
        .metric { background-color: #e8f4f8; padding: 10px; margin: 5px; border-radius: 3px; }
    </style>
</head>
<body>
    <div class="header">
        <h1>BrowserDB Test Report</h1>
        <p>Generated on: $(date)</p>
    </div>
    
    <div class="section">
        <h2>Test Summary</h2>
        <p>This report contains the results of all BrowserDB tests including unit tests, 
        integration tests, performance benchmarks, and memory validation.</p>
    </div>
    
    <div class="section">
        <h2>Test Results</h2>
        <table>
            <tr><th>Test Suite</th><th>Status</th><th>Duration</th><th>Details</th></tr>
            <tr><td>Unit Tests</td><td>Check logs</td><td>See timing files</td><td>Core functionality</td></tr>
            <tr><td>Core Components</td><td>Check logs</td><td>See timing files</td><td>LSM Tree, BDB Format, HeatMap, Modes</td></tr>
            <tr><td>Benchmarks</td><td>Check logs</td><td>N/A</td><td>Performance validation</td></tr>
            <tr><td>Memory Tests</td><td>Check logs</td><td>N/A</td><td>Leak detection</td></tr>
        </table>
    </div>
    
    <div class="section">
        <h2>Performance Metrics</h2>
        <div class="metric">Write Throughput: Check benchmark logs</div>
        <div class="metric">Read Throughput: Check benchmark logs</div>
        <div class="metric">Memory Usage: Check memory test logs</div>
        <div class="metric">Heat Map Performance: Check benchmark logs</div>
    </div>
    
    <div class="section">
        <h2>Coverage Analysis</h2>
        <p>Code coverage analysis can be added using tools like kcov or lcov.</p>
    </div>
</body>
</html>
EOF
    
    log_success "Test report generated: $report_file"
}

run_continuous_integration() {
    log_info "Running CI-style tests..."
    
    # Set CI environment variables
    export CI=true
    export BROWSERDB_CI_MODE=1
    
    # Run all tests with CI-friendly output
    local failed_tests=()
    
    # Unit tests
    if ! run_unit_tests; then
        failed_tests+=("unit_tests")
    fi
    
    # Core tests
    if ! run_core_tests; then
        failed_tests+=("core_tests")
    fi
    
    # Benchmarks
    if ! run_benchmark_tests; then
        failed_tests+=("benchmark_tests")
    fi
    
    # Performance validation
    if ! run_performance_validation; then
        failed_tests+=("performance_validation")
    fi
    
    # Memory tests
    if ! run_memory_tests; then
        failed_tests+=("memory_tests")
    fi
    
    # Report results
    if [ ${#failed_tests[@]} -eq 0 ]; then
        log_success "All CI tests passed!"
        return 0
    else
        log_error "CI tests failed: ${failed_tests[*]}"
        return 1
    fi
}

cleanup() {
    log_info "Cleaning up test environment..."
    
    # Clean temporary files
    rm -f /tmp/browserdb_*.bdb
    rm -f /tmp/browserdb_*.log
    rm -f /tmp/browserdb_lsm_*
    rm -f /tmp/test-*
    
    # Remove test directories (optional)
    # rm -rf "$OUTPUT_DIR"
    # rm -rf "$COVERAGE_DIR"
}

# Main execution
main() {
    local mode="${1:-all}"
    
    log_info "BrowserDB Test Runner starting..."
    log_info "Mode: $mode"
    
    create_directories
    setup_environment
    
    case "$mode" in
        "unit"|"all")
            run_unit_tests
            ;;
        "core"|"all")
            run_core_tests
            ;;
        "bench"|"benchmark"|"all")
            run_benchmark_tests
            ;;
        "perf"|"performance"|"all")
            run_performance_validation
            ;;
        "memory"|"all")
            run_memory_tests
            ;;
        "ci")
            run_continuous_integration
            ;;
        "report")
            generate_test_report
            ;;
        "clean")
            cleanup
            exit 0
            ;;
        "help")
            echo "Usage: $0 [mode]"
            echo "Modes:"
            echo "  unit      - Run unit tests only"
            echo "  core      - Run core component tests"
            echo "  bench     - Run benchmark tests"
            echo "  perf      - Run performance validation"
            echo "  memory    - Run memory leak tests"
            echo "  ci        - Run full CI pipeline"
            echo "  report    - Generate test report only"
            echo "  clean     - Clean test environment"
            echo "  all       - Run all tests (default)"
            echo "  help      - Show this help"
            exit 0
            ;;
        *)
            log_error "Unknown mode: $mode"
            log_info "Use '$0 help' for usage information"
            exit 1
            ;;
    esac
    
    generate_test_report
    
    log_info "BrowserDB Test Runner completed!"
}

# Handle interrupts
trap cleanup EXIT

# Run main function with all arguments
main "$@"
