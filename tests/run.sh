#!/usr/bin/env bash
set -euo pipefail

# =============================================================================
# run.sh — Run functional tests against database containers
#
# Each service in compose.yml uses random port assignment and seeds via
# /docker-entrypoint-initdb.d/ volume mounts.
#
# Usage:
#   ./tests/run.sh                                         # Newest version per backend
#   ./tests/run.sh --all-versions                          # Every Supported version
#   ./tests/run.sh --filter mariadb                        # Newest MariaDB
#   ./tests/run.sh --all-versions --filter mariadb         # Every MariaDB version
#   ./tests/run.sh --all-versions --filter mariadb_10_11   # One specific service
#   ./tests/run.sh --help                                  # Show usage
#
# Environment:
#   TIMEOUT=30      Container readiness timeout in seconds (default: 30)
#   TESTS_PROJECT   Compose project name (default: dbmcp-tests-<checkout hash>)
# =============================================================================

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
COMPOSE_FILE="$SCRIPT_DIR/compose.yml"

# One Compose project per checkout, so runs from different worktrees get their
# own containers, network, and cleanup. TESTS_PROJECT overrides it; an inherited
# COMPOSE_PROJECT_NAME is ignored, since cleanup runs `down -v` on the project.
COMPOSE_PROJECT_NAME="${TESTS_PROJECT:-dbmcp-tests-$(printf '%s' "$SCRIPT_DIR" | sha256sum | cut -c1-8)}"
export COMPOSE_PROJECT_NAME
TIMEOUT="${TIMEOUT:-30}"
TEST_TYPE="${TEST_TYPE:-all}"

# Matrix: service_name:db_type:container_port:newest
#   service_name    — matches compose.yml service; its prefix before `_` is the
#                     DBMCP_DB_BACKEND passed to the functional tests
#   db_type         — mysql, postgres, or sqlite
#   container_port  — internal port to resolve via `docker compose port`
#   newest          — `newest` marks the default-run entry per backend
#
# The versions are edited by hand together with compose.yml and ci.yml.
#
# Test binary names are derived by convention:
#   functional → functional_$type (e.g. "functional_mysql", "functional_sqlite")
#   approval   → approval_$type   (e.g. "approval_mysql", "approval_sqlite")
#   ssh        → functional_ssh for mysql/postgres, run through the sshd service
#   policy     → policy_$type for mysql/postgres: the backend crate's ignored
#                lib tests, which call private policy functions on the database
MATRIX=(
    "mariadb_10_11:mysql:3306:"
    "mariadb_11_4:mysql:3306:"
    "mariadb_11_8:mysql:3306:"
    "mariadb_12_3:mysql:3306:"
    "mariadb_13_0:mysql:3306:newest"
    "mysql_8_4:mysql:3306:"
    "mysql_9_7:mysql:3306:"
    "mysql_26_10:mysql:3306:newest"
    "postgres_15:postgres:5432:"
    "postgres_16:postgres:5432:"
    "postgres_17:postgres:5432:"
    "postgres_18:postgres:5432:"
    "postgres_19:postgres:5432:newest"
    "sqlite:sqlite:0:newest"
)

declare -a RESULTS=()
OVERALL_EXIT=0

cleanup() {
    echo ""
    echo "Cleaning up containers..."
    docker compose -f "$COMPOSE_FILE" down -v --remove-orphans 2>/dev/null || true
}
# Signals exit, so the EXIT trap cleans up once and the run stops.
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

usage() {
    cat <<'EOF'
Usage: ./tests/run.sh [OPTIONS]

Run functional tests against database containers.


Options:
  --all-versions       Run every Supported version, not just the newest
  --filter <pattern>   Run only services matching pattern (substring match)
  --type <type>        Test type: functional, approval, or all (default: all)
  --help               Show this help message

Examples:
  ./tests/run.sh                                         # Newest version per backend
  ./tests/run.sh --all-versions                          # Every Supported version
  ./tests/run.sh --filter postgres                       # Newest PostgreSQL only
  ./tests/run.sh --all-versions --filter postgres        # Every PostgreSQL version
  ./tests/run.sh --all-versions --filter mariadb_10_11   # One specific service
  ./tests/run.sh --filter sqlite                         # SQLite only
  ./tests/run.sh --type functional                       # Functional tests only
  ./tests/run.sh --type approval                         # Approval/snapshot tests only

Environment:
  TIMEOUT=30      Container readiness timeout in seconds (default: 30)
  TESTS_PROJECT   Compose project name (default: dbmcp-tests-<checkout hash>)
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

check_nextest() {
    if ! cargo nextest --version &>/dev/null; then
        echo "ERROR: cargo-nextest is not installed."
        echo "Install it: https://nexte.st/docs/installation/pre-built-binaries/"
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
                    mariadb -h 127.0.0.1 --protocol=TCP -u root app -e "SELECT 1 FROM post_tags LIMIT 1" &>/dev/null 2>&1 \
                || docker compose -f "$COMPOSE_FILE" exec -T "$service" \
                    mysql -h 127.0.0.1 --protocol=TCP -u root app -e "SELECT 1 FROM post_tags LIMIT 1" &>/dev/null 2>&1; then
                    echo " OK (${elapsed}s)"
                    return 0
                fi
                ;;
            postgres)
                if docker compose -f "$COMPOSE_FILE" exec -T "$service" \
                    pg_isready -h 127.0.0.1 -p 5432 -U postgres &>/dev/null 2>&1; then
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

# Starts the sshd service once and waits for its healthcheck.
SSHD_PORT=""
ensure_sshd() {
    [ -n "$SSHD_PORT" ] && return 0

    echo -n "  Starting sshd..."
    if ! docker compose -f "$COMPOSE_FILE" up -d --wait sshd 2>/dev/null; then
        echo " FAILED"
        docker compose -f "$COMPOSE_FILE" logs sshd 2>/dev/null | tail -20
        return 1
    fi
    SSHD_PORT=$(docker compose -f "$COMPOSE_FILE" port sshd 2222 2>/dev/null | cut -d: -f2)
    echo " OK"
}

# Returns space-separated list of test binaries for a given db_type
test_bins_for() {
    local db_type="$1"
    local bins=""
    if [ "$TEST_TYPE" = "all" ] || [ "$TEST_TYPE" = "functional" ]; then
        bins="functional_${db_type}"
        if [ "$db_type" != "sqlite" ]; then
            bins="$bins functional_ssh policy_${db_type}"
        fi
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
    local backend="${service%%_*}"

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

        echo "  Running cargo nextest..."
        for bin in $test_bins; do
            test_output=$(
                DB_PATH="$db_path" \
                MCP_READ_ONLY=false \
                cargo nextest run --workspace --no-fail-fast --ignore-default-filter -E "binary_id(dbmcp::$bin)" 2>&1
            ) || test_exit=$?
            echo "$test_output" | grep -E "^ *(PASS|FAIL|SIG[A-Z]+|ABORT|TIMEOUT|LEAK|Summary) " || true
            local count
            count=$(echo "$test_output" | grep -oP '^ *Summary .* \K\d+(?= passed)' | tail -n 1 || true)
            test_count=$((test_count + ${count:-0}))
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

        echo "  Running cargo nextest..."
        for bin in $test_bins; do
            if [ "$bin" = "functional_ssh" ]; then
                if ! ensure_sshd; then
                    test_exit=1
                    continue
                fi
                # MySQL generates a CA on first boot; its TLS tests skip without one.
                local tls_ca
                local tls_env=()
                tls_ca=$(mktemp)
                if docker compose -f "$COMPOSE_FILE" exec -T "$service" \
                    cat /var/lib/mysql/ca.pem > "$tls_ca" 2>/dev/null && [ -s "$tls_ca" ]; then
                    tls_env=("TLS_TEST_CA=$tls_ca")
                fi
                # DBMCP_DB_HOST is the compose service name: only sshd can resolve it.
                test_output=$(
                    env DBMCP_DB_BACKEND="$db_type" DBMCP_DB_HOST="$service" DBMCP_DB_PORT="$container_port" \
                    DBMCP_SSH_HOST=127.0.0.1 DBMCP_SSH_PORT="$SSHD_PORT" \
                    DBMCP_SSH_USER=dbmcp DBMCP_SSH_PASSWORD=dbmcp \
                    DBMCP_SSH_STRICT_HOST_KEY_CHECKING=no \
                    SSHD_CONTAINER="$(docker compose -f "$COMPOSE_FILE" ps -q sshd)" \
                    ${tls_env[@]+"${tls_env[@]}"} \
                    cargo nextest run --workspace --no-fail-fast --ignore-default-filter -E "binary_id(dbmcp::$bin)" 2>&1
                ) || test_exit=$?
                rm -f "$tls_ca"
            elif [ "$bin" = "policy_${db_type}" ]; then
                test_output=$(
                    DBMCP_DB_BACKEND="$backend" DBMCP_DB_HOST=127.0.0.1 DBMCP_DB_PORT="$host_port" \
                    cargo nextest run --workspace --no-fail-fast -E "package(dbmcp-${db_type}) & kind(lib)" --run-ignored only 2>&1
                ) || test_exit=$?
            else
                test_output=$(
                    DBMCP_DB_BACKEND="$backend" DBMCP_DB_HOST=127.0.0.1 DBMCP_DB_PORT="$host_port" \
                    cargo nextest run --workspace --no-fail-fast --ignore-default-filter -E "binary_id(dbmcp::$bin)" 2>&1
                ) || test_exit=$?
            fi
            echo "$test_output" | grep -E "^ *(PASS|FAIL|SIG[A-Z]+|ABORT|TIMEOUT|LEAK|Summary) " || true
            local count
            count=$(echo "$test_output" | grep -oP '^ *Summary .* \K\d+(?= passed)' | tail -n 1 || true)
            test_count=$((test_count + ${count:-0}))
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
ALL_VERSIONS=false
while [[ $# -gt 0 ]]; do
    case "$1" in
        --all-versions) ALL_VERSIONS=true; shift ;;
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
check_nextest

VERSIONS="newest"
[ "$ALL_VERSIONS" = true ] && VERSIONS="all"
echo "Database Functional Test Suite (type: ${TEST_TYPE}, versions: ${VERSIONS})"
echo "=============================================="
echo "Building project..."
cargo nextest run --workspace --no-run 2>/dev/null || { echo "ERROR: Failed to build test binaries"; exit 2; }

SELECTED=()
for entry in "${MATRIX[@]}"; do
    IFS=':' read -r service _ _ newest <<< "$entry"
    [ "$ALL_VERSIONS" = false ] && [ "$newest" != "newest" ] && continue
    [ -n "$FILTER" ] && [[ "$service" != *"$FILTER"* ]] && continue
    SELECTED+=("$entry")
done

# Pull every selected image at once; compose pulls in parallel.
PULL=()
for entry in "${SELECTED[@]}"; do
    service="${entry%%:*}"
    [ "$service" != sqlite ] && PULL+=("$service")
done
[ ${#PULL[@]} -gt 0 ] && docker compose -f "$COMPOSE_FILE" pull -q "${PULL[@]}" sshd >/dev/null 2>&1 || true

for entry in "${SELECTED[@]}"; do
    IFS=':' read -r service db_type container_port _ <<< "$entry"
    run_entry "$service" "$db_type" "$container_port"
done

if [ ${#RESULTS[@]} -eq 0 ]; then
    echo "No matrix entries matched filter: $FILTER"
    [ "$ALL_VERSIONS" = false ] && echo "Only the newest version per backend runs by default; add --all-versions to match older ones."
    exit 1
fi

print_summary
exit $OVERALL_EXIT
