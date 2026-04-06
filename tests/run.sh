#!/usr/bin/env bash
set -euo pipefail

# =============================================================================
# run.sh — Run functional tests against database containers
#
# Each service in compose.yml uses random port assignment and seeds via
# /docker-entrypoint-initdb.d/ volume mounts.
#
# Usage:
#   ./tests/run.sh                     # Run full matrix
#   ./tests/run.sh --filter mariadb    # All MariaDB services
#   ./tests/run.sh --filter mysql_9    # Specific service
#   ./tests/run.sh --help              # Show usage
#
# Environment:
#   TIMEOUT=30   Container readiness timeout in seconds (default: 30)
# =============================================================================

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
COMPOSE_FILE="$SCRIPT_DIR/compose.yml"
TIMEOUT="${TIMEOUT:-30}"
TEST_TYPE="${TEST_TYPE:-all}"

# Matrix: service_name:db_type:container_port
#   service_name    — matches compose.yml service
#   db_type         — mysql, postgres, or sqlite
#   container_port  — internal port to resolve via `docker compose port`
#
# Test binary names are derived by convention:
#   functional → functional_$type (e.g. "functional_mysql", "functional_sqlite")
#   approval   → approval_$type   (e.g. "approval_mysql", "approval_sqlite")
MATRIX=(
    "mariadb_12:mysql:3306"
    "mysql_9:mysql:3306"
    "postgres_18:postgres:5432"
    "sqlite:sqlite:0"
)

declare -a RESULTS=()
OVERALL_EXIT=0

cleanup() {
    echo ""
    echo "Cleaning up containers..."
    docker compose -f "$COMPOSE_FILE" down -v --remove-orphans 2>/dev/null || true
}
trap cleanup EXIT INT TERM

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

usage() {
    cat <<'EOF'
Usage: ./tests/run.sh [OPTIONS]

Run functional tests against database containers.

Options:
  --filter <pattern>   Run only services matching pattern (substring match)
  --type <type>        Test type: functional, approval, or all (default: all)
  --help               Show this help message

Examples:
  ./tests/run.sh                          # Full matrix (all test types)
  ./tests/run.sh --filter mariadb         # All MariaDB services
  ./tests/run.sh --filter postgres        # All PostgreSQL services
  ./tests/run.sh --filter sqlite          # SQLite only
  ./tests/run.sh --type functional         # Functional tests only
  ./tests/run.sh --type approval          # Approval/snapshot tests only

Environment:
  TIMEOUT=30   Container readiness timeout in seconds (default: 30)
EOF
}

check_docker() {
    if ! command -v docker &>/dev/null; then
        echo "ERROR: Docker is not installed or not in PATH."
        echo "Install Docker: https://docs.docker.com/get-docker/"
        exit 2
    fi
    if ! docker info &>/dev/null; then
        echo "ERROR: Docker daemon is not running."
        echo "Start Docker and try again."
        exit 2
    fi
}

wait_for_ready() {
    local service="$1"
    local db_type="$2"
    local elapsed=0

    echo -n "  Waiting for readiness..."
    while [ $elapsed -lt "$TIMEOUT" ]; do
        case "$db_type" in
            mysql)
                if docker compose -f "$COMPOSE_FILE" exec -T "$service" \
                    mariadb -u root app -e "SELECT 1 FROM post_tags LIMIT 1" &>/dev/null 2>&1 \
                || docker compose -f "$COMPOSE_FILE" exec -T "$service" \
                    mysql -u root app -e "SELECT 1 FROM post_tags LIMIT 1" &>/dev/null 2>&1; then
                    echo " OK (${elapsed}s)"
                    return 0
                fi
                ;;
            postgres)
                if docker compose -f "$COMPOSE_FILE" exec -T "$service" \
                    pg_isready -U postgres &>/dev/null 2>&1; then
                    echo " OK (${elapsed}s)"
                    return 0
                fi
                ;;
        esac
        sleep 1
        elapsed=$((elapsed + 1))
    done

    echo " TIMEOUT after ${TIMEOUT}s"
    return 1
}

# Returns space-separated list of test binaries for a given db_type
test_bins_for() {
    local db_type="$1"
    local bins=""
    if [ "$TEST_TYPE" = "all" ] || [ "$TEST_TYPE" = "functional" ]; then
        bins="functional_${db_type}"
    fi
    if [ "$TEST_TYPE" = "all" ] || [ "$TEST_TYPE" = "approval" ]; then
        bins="${bins:+$bins }approval_${db_type}"
    fi
    echo "$bins"
}

# ---------------------------------------------------------------------------
# Run one matrix entry
# ---------------------------------------------------------------------------

run_entry() {
    local service="$1"
    local db_type="$2"
    local container_port="$3"

    local test_bins
    test_bins=$(test_bins_for "$db_type")

    echo ""
    echo "=== Testing ${service} (${TEST_TYPE}) ==="
    local start_time
    start_time=$(date +%s)

    local test_exit=0
    local test_output
    local test_count=0

    if [ "$db_type" = "sqlite" ]; then
        local db_path="${SCRIPT_DIR}/database.db"

        echo -n "  Generating database via Docker Compose..."
        if ! DOCKER_UID="$(id -u)" DOCKER_GID="$(id -g)" \
            docker compose -f "$COMPOSE_FILE" run --rm sqlite 2>/dev/null; then
            echo " FAILED"
            RESULTS+=("${service}|SKIP|0|$(( $(date +%s) - start_time ))")
            OVERALL_EXIT=1; return
        fi
        echo " OK"

        echo "  Running cargo test..."
        for bin in $test_bins; do
            test_output=$(
                DB_PATH="$db_path" \
                MCP_READ_ONLY=false \
                cargo test --test "$bin" -- --test-threads=1 2>&1
            ) || test_exit=$?
            echo "$test_output" | grep -E "^(test |test result:)" || true
            local count
            count=$(echo "$test_output" | grep -oP '\d+ passed' | grep -oP '\d+' || echo "0")
            test_count=$((test_count + count))
        done

        rm -f "$db_path"
    else
        echo -n "  Starting container..."
        if ! docker compose -f "$COMPOSE_FILE" up -d "$service" 2>/dev/null; then
            echo " FAILED"
            RESULTS+=("${service}|SKIP|0|$(( $(date +%s) - start_time ))")
            OVERALL_EXIT=1; return
        fi
        echo " OK"

        local host_port
        host_port=$(docker compose -f "$COMPOSE_FILE" port "$service" "$container_port" 2>/dev/null | cut -d: -f2)

        if ! wait_for_ready "$service" "$db_type"; then
            echo "  Container failed to become healthy. Logs:"
            docker compose -f "$COMPOSE_FILE" logs "$service" 2>/dev/null | tail -20
            docker compose -f "$COMPOSE_FILE" stop "$service" 2>/dev/null || true
            docker compose -f "$COMPOSE_FILE" rm -f -v "$service" 2>/dev/null || true
            RESULTS+=("${service}|SKIP|0|$(( $(date +%s) - start_time ))")
            OVERALL_EXIT=1; return
        fi

        # Build DATABASE_URL for #[sqlx::test] macro (approval tests)
        local database_url
        case "$db_type" in
            mysql)    database_url="mysql://root@127.0.0.1:${host_port}/mysql" ;;
            postgres) database_url="postgres://postgres@127.0.0.1:${host_port}/postgres" ;;
        esac

        echo "  Running cargo test..."
        for bin in $test_bins; do
            test_output=$(
                DB_HOST=127.0.0.1 DB_PORT="$host_port" \
                DATABASE_URL="$database_url" \
                cargo test --test "$bin" -- --test-threads=1 2>&1
            ) || test_exit=$?
            echo "$test_output" | grep -E "^(test |test result:)" || true
            local count
            count=$(echo "$test_output" | grep -oP '\d+ passed' | grep -oP '\d+' || echo "0")
            test_count=$((test_count + count))
        done

        echo -n "  Stopping container..."
        docker compose -f "$COMPOSE_FILE" stop "$service" 2>/dev/null || true
        docker compose -f "$COMPOSE_FILE" rm -f -v "$service" 2>/dev/null || true
        echo " OK"
    fi

    local duration=$(( $(date +%s) - start_time ))
    if [ "$test_exit" -eq 0 ]; then
        RESULTS+=("${service}|PASS|${test_count}|${duration}")
    else
        RESULTS+=("${service}|FAIL|${test_count}|${duration}")
        OVERALL_EXIT=1
        echo "  FAILED — see output above"
    fi
}

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------

print_summary() {
    echo ""
    echo "╔══════════════════╦════════╦═══════╦══════════╗"
    echo "║ Service          ║ Status ║ Tests ║ Duration ║"
    echo "╠══════════════════╬════════╬═══════╬══════════╣"

    local total_tests=0 total_duration=0 fail_count=0 entry_count=0

    for result in "${RESULTS[@]}"; do
        IFS='|' read -r svc status tests duration <<< "$result"
        printf "║ %-16s ║ %-6s ║ %-5s ║ %6ss ║\n" "$svc" "$status" "$tests" "$duration"
        total_tests=$((total_tests + tests))
        total_duration=$((total_duration + duration))
        entry_count=$((entry_count + 1))
        [ "$status" != "PASS" ] && fail_count=$((fail_count + 1))
    done

    echo "╠══════════════════╬════════╬═══════╬══════════╣"

    local overall="PASS"
    [ "$fail_count" -gt 0 ] && overall="${fail_count} FAIL"
    local dfmt="${total_duration}s"
    [ "$total_duration" -ge 60 ] && dfmt="$((total_duration / 60))m $((total_duration % 60))s"

    printf "║ %-16s ║ %-6s ║ %-5s ║ %6s  ║\n" "TOTAL ($entry_count)" "$overall" "$total_tests" "$dfmt"
    echo "╚══════════════════╩════════╩═══════╩══════════╝"
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

FILTER=""
while [[ $# -gt 0 ]]; do
    case "$1" in
        --filter) FILTER="$2"; shift 2 ;;
        --type) TEST_TYPE="$2"; shift 2 ;;
        --help|-h) usage; exit 0 ;;
        *) echo "Unknown option: $1"; usage; exit 1 ;;
    esac
done

case "$TEST_TYPE" in
    functional|approval|all) ;;
    *) echo "ERROR: --type must be one of: functional, approval, all"; exit 1 ;;
esac

check_docker

echo "Database Functional Test Suite (type: ${TEST_TYPE})"
echo "=============================================="
echo "Building project..."
cargo test --no-run 2>/dev/null || { echo "ERROR: Failed to build test binaries"; exit 2; }

for entry in "${MATRIX[@]}"; do
    IFS=':' read -r service db_type container_port <<< "$entry"
    [ -n "$FILTER" ] && [[ "$service" != *"$FILTER"* ]] && continue
    run_entry "$service" "$db_type" "$container_port"
done

if [ ${#RESULTS[@]} -eq 0 ]; then
    echo "No matrix entries matched filter: $FILTER"
    exit 1
fi

print_summary
exit $OVERALL_EXIT
