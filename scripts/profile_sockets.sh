#!/usr/bin/env bash
# profile_sockets.sh - Diagnostic socket and buffer inspection for mm-dlp on Linux.
set -euo pipefail

echo "=== Linux TCP & Socket Performance Diagnostics for mm-dlp ==="

echo "1. Kernel Socket Buffer Limits (sysctl):"
sysctl net.core.rmem_max net.core.wmem_max net.ipv4.tcp_rmem net.ipv4.tcp_wmem net.ipv4.tcp_congestion_control net.ipv4.tcp_fastopen || true

echo ""
echo "2. Active mm-dlp TCP Sockets & Buffer Depths:"
ss -tiepm state established '( dport = :http or dport = :https )' | grep -A 1 -E "mm-dlp|curl" || echo "No active mm-dlp socket connections found."

echo ""
echo "3. Network Interface Drop & Error Counters:"
netstat -s | grep -i -E "drop|overflow|retransmit|failed" | head -n 20 || ip -s link

echo ""
echo "=== Diagnostics complete ==="
