#!/usr/bin/env bash
set -euo pipefail
class=${1:-interactive}
shift || true
case $class in interactive|build|browser-test|batch) ;; *) echo "invalid workload class" >&2; exit 2 ;; esac
if (( $# == 0 )); then
  exec ramshared session --class "$class"
fi
exec ramshared run --class "$class" -- "$@"
