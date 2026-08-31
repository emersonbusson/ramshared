#!/bin/bash
HEADERS_DIR=$(find /usr/src -maxdepth 1 -name "linux-headers-*-generic" | head -n 1)
gcc -fsyntax-only -Wall -Wextra -I $HEADERS_DIR/include -I $HEADERS_DIR/arch/x86/include drivers/block/ramshared/*.c
