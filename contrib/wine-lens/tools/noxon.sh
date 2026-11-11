#!/bin/sh
# SPDX-License-Identifier: MIT
# Keep software flow control off on the lens board's port while Leia's service runs under Wine: Wine turns
# IXON on when a program asks for XON/XOFF, which eats bytes of the board's replies; Windows' USB serial driver
# never acts on those bytes. Changes line settings only; reads and writes nothing. Runs until killed.
while :; do
    stty -F /dev/odyssey-lens -a 2>/dev/null | grep -q -E '(^| )ixon' && stty -F /dev/odyssey-lens -ixon -ixoff 2>/dev/null
    sleep 0.05
done
