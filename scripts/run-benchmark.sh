#!/bin/bash
#
# OBS FUSE Performance Benchmark Runner
#
# Usage:
#   ./scripts/run-benchmark.sh              # Normal mode (~10 min)
#   ./scripts/run-benchmark.sh --quick      # Quick mode (~2 min)
#   ./scripts/run-benchmark.sh --full       # Full mode with regression check (~30 min)
#   ./scripts/run-benchmark.sh --with-obs   # Include OBS integration tests
#

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
REPORT_DIR="$PROJECT_DIR/test-reports"
TIMESTAMP=$(date +"%Y%m%d_%H%M%S")

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Parse arguments
WITH_OBS=false
BENCH_MODE="normal"

for arg in "$@"; do
    case $arg in
        --quick)
            BENCH_MODE="quick"
            shift
            ;;
        --full)
            BENCH_MODE="full"
            WITH_OBS=true
            shift
            ;;
        --with-obs)
            WITH_OBS=true
            shift
            ;;
        *)
            # Pass through to cargo bench
            ;;
    esac
done

export BENCH_MODE

echo -e "${BLUE}╔════════════════════════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║          OBS FUSE Performance Benchmark Suite              ║${NC}"
echo -e "${BLUE}╚════════════════════════════════════════════════════════════╝${NC}"
echo ""
echo -e "  Mode: ${YELLOW}$BENCH_MODE${NC}"
echo ""

# Create report directory if not exists
mkdir -p "$REPORT_DIR"
mkdir -p "$REPORT_DIR/history"

# Collect system info
echo -e "${YELLOW}▶ Collecting system information...${NC}"
OS_NAME=$(uname -s)
OS_VERSION=$(uname -r)
CPU_INFO=$(sysctl -n machdep.cpu.brand_string 2>/dev/null || cat /proc/cpuinfo 2>/dev/null | grep "model name" | head -1 | cut -d: -f2 || echo "Unknown")
MEMORY_INFO=$(sysctl -n hw.memsize 2>/dev/null | awk '{print $1/1024/1024/1024 " GB"}' || free -h 2>/dev/null | grep Mem | awk '{print $2}' || echo "Unknown")

cat > "$REPORT_DIR/system-info.json" << EOF
{
    "timestamp": "$TIMESTAMP",
    "os": "$OS_NAME",
    "os_version": "$OS_VERSION",
    "cpu": "$CPU_INFO",
    "memory": "$MEMORY_INFO",
    "rust_version": "$(rustc --version)"
}
EOF

echo -e "  OS: $OS_NAME $OS_VERSION"
echo -e "  CPU: $CPU_INFO"
echo -e "  Memory: $MEMORY_INFO"
echo ""

# Run consistency tests first
echo -e "${YELLOW}▶ Running consistency tests...${NC}"
if cargo test --test consistency 2>&1 | tee "$REPORT_DIR/consistency-test.log"; then
    echo -e "${GREEN}  ✓ All consistency tests passed${NC}"
else
    echo -e "${RED}  ✗ Consistency tests failed!${NC}"
    exit 1
fi
echo ""

# Run local benchmarks
echo -e "${YELLOW}▶ Running local component benchmarks...${NC}"
CRITERION_DIR="$PROJECT_DIR/target/criterion"

if [ "$WITH_OBS" = true ]; then
    # Check for OBS credentials
    if [ -f "$PROJECT_DIR/.obs-credentials" ]; then
        source "$PROJECT_DIR/.obs-credentials"
    fi

    if [ -z "$OBS_ACCESS_KEY" ] || [ -z "$OBS_SECRET_KEY" ]; then
        echo -e "${RED}  ✗ OBS credentials not found. Set OBS_ACCESS_KEY and OBS_SECRET_KEY${NC}"
        echo -e "${YELLOW}  Running local benchmarks only...${NC}"
        cargo bench --bench benchmark 2>&1 | tee "$REPORT_DIR/benchmark.log"
    else
        echo -e "${GREEN}  ✓ OBS credentials found, including integration tests${NC}"
        cargo bench --bench benchmark --features obs-bench 2>&1 | tee "$REPORT_DIR/benchmark.log"
    fi
else
    cargo bench --bench benchmark 2>&1 | tee "$REPORT_DIR/benchmark.log"
fi

# Copy Criterion HTML reports
if [ -d "$CRITERION_DIR" ]; then
    echo -e "${YELLOW}▶ Copying HTML reports...${NC}"
    rm -rf "$REPORT_DIR/criterion-report"
    cp -r "$CRITERION_DIR" "$REPORT_DIR/criterion-report"
    echo -e "${GREEN}  ✓ HTML reports saved to test-reports/criterion-report/${NC}"
fi

# Generate JSON data
echo -e "${YELLOW}▶ Generating JSON benchmark data...${NC}"
"$SCRIPT_DIR/generate-report.sh" json

# Generate Markdown report
echo -e "${YELLOW}▶ Generating Markdown report...${NC}"
"$SCRIPT_DIR/generate-report.sh" markdown

# Save to history for regression detection
if [ "$BENCH_MODE" = "full" ]; then
    echo -e "${YELLOW}▶ Saving to history for regression detection...${NC}"
    cp "$REPORT_DIR/benchmark-data.json" "$REPORT_DIR/history/benchmark-$TIMESTAMP.json"

    # Check for regression against baseline
    if [ -f "$REPORT_DIR/baseline.json" ]; then
        "$SCRIPT_DIR/check-regression.sh"
    else
        echo -e "${YELLOW}  No baseline.json found. Creating baseline...${NC}"
        cp "$REPORT_DIR/benchmark-data.json" "$REPORT_DIR/baseline.json"
    fi
fi

echo ""
echo -e "${GREEN}╔════════════════════════════════════════════════════════════╗${NC}"
echo -e "${GREEN}║                    Benchmark Complete!                      ║${NC}"
echo -e "${GREEN}╚════════════════════════════════════════════════════════════╝${NC}"
echo ""
echo -e "Reports generated:"
echo -e "  ${BLUE}• Markdown:${NC} test-reports/benchmark-report.md"
echo -e "  ${BLUE}• JSON:${NC}     test-reports/benchmark-data.json"
echo -e "  ${BLUE}• HTML:${NC}     test-reports/criterion-report/report/index.html"
echo ""
