#!/usr/bin/env bash
# Simulation script for the data privacy VHS GIF.
# Runs as an interactive session that looks like Claude Code, with redaction and the
# optional ML model on, so names come back as <PERSON> too.
# Usage: ./privacy.sh (interactive mode, no arguments)

set -euo pipefail

ACCENT='\033[38;2;143;138;251m'
GREEN='\033[38;2;66;190;101m'
FG='\033[38;2;230;233;239m'
BOLD='\033[1m'
DIM='\033[2m'
RESET='\033[0m'

# Simulate Claude Code thinking animation
think() {
  local duration="$1"
  local total="$2"
  local flowers=("✻" "✼" "✽" "✾" "✿" "❀" "❁" "✿" "✾" "✽" "✼" "✻")
  local steps=$(echo "$total / 0.15" | bc)
  local i=0

  for _ in $(seq 1 "$steps"); do
    local idx=$(( i % ${#flowers[@]} ))
    printf "\r${FG}${flowers[$idx]}${RESET} ${DIM}Thinking...${RESET} "
    sleep 0.15
    i=$((i + 1))
  done
  printf "\r${FG}✻${RESET} ${DIM}Crunched in ${duration}${RESET}   \n"
  sleep 0.2
}

respond() {
  local query="$1"
  printf "\n"
  case "$query" in
    *"biggest orders"*)
      think "2.1s" 2.1
      printf "\n${GREEN}●${RESET} ${BOLD}readQuery${RESET} ${DIM}(dbmcp)${RESET}\n"
      printf "  ${DIM}⎿  3 rows, personal data redacted${RESET}\n"
      sleep 0.4
      printf "\n● Last week's 3 biggest orders:\n\n"
      printf "  #1042  ${ACCENT}<PERSON>  <EMAIL_ADDRESS>${RESET}  \$4,820.00\n"
      printf "  #1037  ${ACCENT}<PERSON>  <EMAIL_ADDRESS>${RESET}  \$3,915.50\n"
      printf "  #1051  ${ACCENT}<PERSON>  <EMAIL_ADDRESS>${RESET}  \$2,640.00\n"
      ;;
    *"who placed"*|*"Who placed"*)
      think "1.4s" 1.4
      printf "\n● I can't tell: dbmcp redacted the real values,\n"
      printf "  so I only see ${ACCENT}<PERSON>${RESET} and ${ACCENT}<EMAIL_ADDRESS>${RESET}.\n"
      printf "  The total, ${BOLD}\$4,820.00${RESET}, is not personal data.\n"
      ;;
    *)
      think "1.0s" 1.0
      printf "\n● I can help with questions about your database.\n"
      ;;
  esac
  printf "\n"
}

# Clear screen to hide launch command in VHS recordings
clear

# Interactive loop with Claude Code-style prompt
while true; do
  printf "${BOLD}${FG}>${RESET} "
  read -r line || break
  if [[ -z "$line" ]]; then
    continue
  fi
  respond "$line"
done
