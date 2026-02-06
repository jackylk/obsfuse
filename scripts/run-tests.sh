#!/bin/bash
#
# OBS FUSE Complete Test Runner
#
# Usage:
#   ./scripts/run-tests.sh                  # Run all tests (functional + benchmark)
#   ./scripts/run-tests.sh --quick          # Quick mode
#   ./scripts/run-tests.sh --functional     # Only functional tests
#   ./scripts/run-tests.sh --benchmark      # Only benchmark tests
#   ./scripts/run-tests.sh --with-obs       # Include OBS integration tests
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
NC='\033[0m'

# Parse arguments
RUN_FUNCTIONAL=true
RUN_BENCHMARK=true
WITH_OBS=false
BENCH_MODE="normal"

for arg in "$@"; do
    case $arg in
        --quick)
            BENCH_MODE="quick"
            ;;
        --full)
            BENCH_MODE="full"
            WITH_OBS=true
            ;;
        --functional)
            RUN_BENCHMARK=false
            ;;
        --benchmark)
            RUN_FUNCTIONAL=false
            ;;
        --with-obs)
            WITH_OBS=true
            ;;
    esac
done

export BENCH_MODE

echo -e "${BLUE}╔════════════════════════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║            OBS FUSE Complete Test Suite                    ║${NC}"
echo -e "${BLUE}╚════════════════════════════════════════════════════════════╝${NC}"
echo ""
echo -e "  Mode: ${YELLOW}$BENCH_MODE${NC}"
echo -e "  Functional: $([ "$RUN_FUNCTIONAL" = true ] && echo "${GREEN}yes${NC}" || echo "${YELLOW}no${NC}")"
echo -e "  Benchmark: $([ "$RUN_BENCHMARK" = true ] && echo "${GREEN}yes${NC}" || echo "${YELLOW}no${NC}")"
echo -e "  OBS Integration: $([ "$WITH_OBS" = true ] && echo "${GREEN}yes${NC}" || echo "${YELLOW}no${NC}")"
echo ""

# Create report directory
mkdir -p "$REPORT_DIR"
mkdir -p "$REPORT_DIR/history"

# Load OBS credentials if needed
if [ "$WITH_OBS" = true ] && [ -f "$PROJECT_DIR/.obs-credentials" ]; then
    source "$PROJECT_DIR/.obs-credentials"
fi

# ============================================================================
# Consistency Tests (always run)
# ============================================================================

echo -e "${YELLOW}▶ Running consistency tests...${NC}"
if cargo test --test consistency 2>&1 | tee "$REPORT_DIR/consistency-test.log"; then
    echo -e "${GREEN}  ✓ All consistency tests passed${NC}"
    CONSISTENCY_RESULT="passed"
else
    echo -e "${RED}  ✗ Consistency tests failed!${NC}"
    CONSISTENCY_RESULT="failed"
fi
echo ""

# ============================================================================
# Functional Tests
# ============================================================================

if [ "$RUN_FUNCTIONAL" = true ]; then
    echo -e "${YELLOW}▶ Running functional tests...${NC}"

    if [ "$WITH_OBS" = true ] && [ -n "$OBS_ACCESS_KEY" ]; then
        if cargo test --test functional 2>&1 | tee "$REPORT_DIR/functional-test.log"; then
            echo -e "${GREEN}  ✓ All functional tests passed${NC}"
            FUNCTIONAL_RESULT="passed"
        else
            echo -e "${RED}  ✗ Some functional tests failed${NC}"
            FUNCTIONAL_RESULT="failed"
        fi
    else
        echo -e "${YELLOW}  ⚠ Skipping functional tests (OBS credentials required)${NC}"
        FUNCTIONAL_RESULT="skipped"
    fi
    echo ""
fi

# ============================================================================
# Benchmark Tests
# ============================================================================

if [ "$RUN_BENCHMARK" = true ]; then
    echo -e "${YELLOW}▶ Running benchmark tests (mode: $BENCH_MODE)...${NC}"

    CRITERION_DIR="$PROJECT_DIR/target/criterion"

    if [ "$WITH_OBS" = true ] && [ -n "$OBS_ACCESS_KEY" ]; then
        cargo bench --bench benchmark --features obs-bench 2>&1 | tee "$REPORT_DIR/benchmark.log"
    else
        cargo bench --bench benchmark 2>&1 | tee "$REPORT_DIR/benchmark.log"
    fi

    BENCHMARK_RESULT="completed"

    # Copy Criterion HTML reports
    if [ -d "$CRITERION_DIR" ]; then
        rm -rf "$REPORT_DIR/criterion-report"
        cp -r "$CRITERION_DIR" "$REPORT_DIR/criterion-report"
        echo -e "${GREEN}  ✓ HTML reports saved${NC}"
    fi

    # Generate reports
    "$SCRIPT_DIR/generate-report.sh" json 2>/dev/null || true
    "$SCRIPT_DIR/generate-report.sh" markdown 2>/dev/null || true

    echo ""
fi

# ============================================================================
# Generate Summary Report
# ============================================================================

echo -e "${YELLOW}▶ Generating summary report...${NC}"

cat > "$REPORT_DIR/test-summary.md" << EOF
# OBS FUSE Test Summary

**Generated:** $(date +"%Y-%m-%d %H:%M:%S")
**Mode:** $BENCH_MODE

## Test Results

| Test Suite | Status |
|------------|--------|
| Consistency Tests | $CONSISTENCY_RESULT |
EOF

if [ "$RUN_FUNCTIONAL" = true ]; then
    echo "| Functional Tests | $FUNCTIONAL_RESULT |" >> "$REPORT_DIR/test-summary.md"
fi

if [ "$RUN_BENCHMARK" = true ]; then
    echo "| Benchmark Tests | $BENCHMARK_RESULT |" >> "$REPORT_DIR/test-summary.md"
fi

cat >> "$REPORT_DIR/test-summary.md" << EOF

## Report Files

- Consistency: \`test-reports/consistency-test.log\`
EOF

if [ "$RUN_FUNCTIONAL" = true ]; then
    echo "- Functional: \`test-reports/functional-test.log\`" >> "$REPORT_DIR/test-summary.md"
fi

if [ "$RUN_BENCHMARK" = true ]; then
    cat >> "$REPORT_DIR/test-summary.md" << EOF
- Benchmark Log: \`test-reports/benchmark.log\`
- Benchmark Report: \`test-reports/benchmark-report.md\`
- Benchmark Data: \`test-reports/benchmark-data.json\`
- HTML Report: \`test-reports/criterion-report/report/index.html\`
EOF
fi

# Generate JSON summary
cat > "$REPORT_DIR/test-summary.json" << EOF
{
    "timestamp": "$(date -u +"%Y-%m-%dT%H:%M:%SZ")",
    "mode": "$BENCH_MODE",
    "results": {
        "consistency": "$CONSISTENCY_RESULT",
        "functional": "${FUNCTIONAL_RESULT:-not_run}",
        "benchmark": "${BENCHMARK_RESULT:-not_run}"
    }
}
EOF

echo ""
echo -e "${GREEN}╔════════════════════════════════════════════════════════════╗${NC}"
echo -e "${GREEN}║                    Test Suite Complete!                     ║${NC}"
echo -e "${GREEN}╚════════════════════════════════════════════════════════════╝${NC}"
echo ""
echo -e "Results:"
echo -e "  ${BLUE}• Consistency:${NC} $CONSISTENCY_RESULT"
[ "$RUN_FUNCTIONAL" = true ] && echo -e "  ${BLUE}• Functional:${NC} $FUNCTIONAL_RESULT"
[ "$RUN_BENCHMARK" = true ] && echo -e "  ${BLUE}• Benchmark:${NC} $BENCHMARK_RESULT"
echo ""
echo -e "Reports saved to: ${YELLOW}test-reports/${NC}"
echo ""
