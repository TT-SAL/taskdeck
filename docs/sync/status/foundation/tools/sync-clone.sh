#!/bin/bash
# After a commit in the main checkout: move a working copy's HEAD to it and refresh, in its working tree,
# the paths its agents never write. Never touches the files agents are working on.  usage: sync-clone.sh c|s
set -euo pipefail
C=/home/user/taskdeck/target/work/$1
B=$(git -C /home/user/taskdeck rev-parse --abbrev-ref HEAD)
git -C "$C" fetch -q origin
git -C "$C" reset -q --mixed "origin/$B"
if [ "$1" = c ]; then
    git -C "$C" checkout -q HEAD -- docs/sync/status docs/sync/spec     # code agents write neither
else
    git -C "$C" checkout -q HEAD -- . ':!docs/sync/spec'                 # spec agents write only docs/sync/spec
fi
git -C "$C" status --short
