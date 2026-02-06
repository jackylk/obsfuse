#!/bin/bash
#
# Check for performance regression against baseline
#
# Exit codes:
#   0 - No regression
#   1 - Warning (>10% slower)
#   2 - Failure (>20% slower)
#

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
REPORT_DIR="$PROJECT_DIR/test-reports"

BASELINE="$REPORT_DIR/baseline.json"
CURRENT="$REPORT_DIR/benchmark-data.json"

WARN_THRESHOLD=10
FAIL_THRESHOLD=20

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

if [ ! -f "$BASELINE" ]; then
    echo -e "${YELLOW}No baseline found. Skipping regression check.${NC}"
    exit 0
fi

if [ ! -f "$CURRENT" ]; then
    echo -e "${RED}Current benchmark data not found.${NC}"
    exit 1
fi

echo "Checking for performance regression..."
echo ""

warnings=0
failures=0

# Compare each benchmark
for bench in $(jq -r '.benchmarks | keys[]' "$BASELINE" 2>/dev/null); do
    baseline_mean=$(jq -r ".benchmarks[\"$bench\"].mean_ns // 0" "$BASELINE")
    current_mean=$(jq -r ".benchmarks[\"$bench\"].mean_ns // 0" "$CURRENT")

    if [ "$baseline_mean" = "0" ] || [ "$baseline_mean" = "null" ]; then
        continue
    fi

    if [ "$current_mean" = "0" ] || [ "$current_mean" = "null" ]; then
        echo -e "${YELLOW}  [SKIP] $bench - no current data${NC}"
        continue
    fi

    # Calculate percentage change
    change=$(echo "scale=2; (($current_mean - $baseline_mean) / $baseline_mean) * 100" | bc)
    change_abs=$(echo "$change" | tr -d '-')

    if (( $(echo "$change > $FAIL_THRESHOLD" | bc -l) )); then
        echo -e "${RED}  [FAIL] $bench: ${change}% slower${NC}"
        ((failures++))
    elif (( $(echo "$change > $WARN_THRESHOLD" | bc -l) )); then
        echo -e "${YELLOW}  [WARN] $bench: ${change}% slower${NC}"
        ((warnings++))
    elif (( $(echo "$change < -$WARN_THRESHOLD" | bc -l) )); then
        echo -e "${GREEN}  [IMPR] $bench: ${change}% faster${NC}"
    else
        echo -e "  [OK]   $bench: ${change}%"
    fi
done

echo ""

if [ $failures -gt 0 ]; then
    echo -e "${RED}✗ $failures benchmark(s) failed regression check (>${FAIL_THRESHOLD}% slower)${NC}"
    exit 2
elif [ $warnings -gt 0 ]; then
    echo -e "${YELLOW}⚠ $warnings benchmark(s) have warnings (>${WARN_THRESHOLD}% slower)${NC}"
    exit 1
else
    echo -e "${GREEN}✓ No performance regression detected${NC}"
    exit 0
fi
