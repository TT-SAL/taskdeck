#!/bin/bash
# After a commit in the main checkout: move a working copy's HEAD to it and refresh the lead's own
# paths there. Never touches the files agents are working on.  usage: sync-clone.sh c|s
set -euo pipefail
C=/home/user/taskdeck/target/work/$1
B=$(git -C /home/user/taskdeck rev-parse --abbrev-ref HEAD)
git -C "$C" fetch -q origin
git -C "$C" reset -q --mixed "origin/$B"
git -C "$C" checkout -q HEAD -- docs/sync/status
git -C "$C" status --short
