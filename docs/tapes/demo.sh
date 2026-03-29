#!/usr/bin/env bash
# Simulation script for VHS demo GIF.
# Runs as an interactive session that looks like Claude Code.
# Usage: ./demo-sim.sh (interactive mode, no arguments)

set -euo pipefail

ORANGE='\033[38;2;255;72;0m'
GREEN='\033[38;2;22;163;74m'
BLACK='\033[38;2;26;26;26m'
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
    printf "\r${BLACK}${flowers[$idx]}${RESET} ${DIM}Thinking...${RESET} "
    sleep 0.15
    i=$((i + 1))
  done
  printf "\r${BLACK}✻${RESET} ${DIM}Crunched in ${duration}${RESET}   \n"
  sleep 0.2
}

respond() {
  local query="$1"
  printf "\n"
  case "$query" in
    *"how many users"*|*"How many users"*)
      think "1.8s" 1.8
      printf "\n● ${BOLD}${ORANGE}847${RESET} users are registered in the database.\n"
      ;;
    *"most posts"*|*"wrote the most"*)
      think "3.2s" 3.2
      printf "\n● ${BOLD}${ORANGE}Sarah Chen${RESET} with ${GREEN}142${RESET} posts.\n"
      ;;
    *"recent"*|*"latest"*)
      think "2.4s" 2.4
      printf "\n● The 3 most recent posts:\n"
      printf "\n  ${DIM}1.${RESET} ${BOLD}Optimizing database queries${RESET} — Sarah Chen, 2h ago\n"
      printf "  ${DIM}2.${RESET} ${BOLD}Getting started with MCP${RESET} — Alex Rivera, 5h ago\n"
      printf "  ${DIM}3.${RESET} ${BOLD}PostgreSQL tips and tricks${RESET} — Jordan Lee, 1d ago\n"
      ;;
    *)
      think "1.0s" 1.0
      printf "\n● I can help with questions about your PostgreSQL database.\n"
      ;;
  esac
  printf "\n"
}

# Clear screen to hide launch command in VHS recordings
clear

# Interactive loop with Claude Code-style prompt
while true; do
  printf "${BOLD}${BLACK}>${RESET} "
  read -r line || break
  if [[ -z "$line" ]]; then
    continue
  fi
  respond "$line"
done
