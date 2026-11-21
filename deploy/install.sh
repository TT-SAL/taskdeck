#!/bin/sh
# Install taskdeck-server as a service on a headless Linux box — SERVER.md §5,
# as one command. Safe to run again: every step checks before it acts.
#
#   cargo build --release --bin taskdeck-server
#   sudo deploy/install.sh
#
# What it does, in order: a system user `taskdeck` with /var/lib/taskdeck as
# its home; /var/lib/taskdeck/taskdeck_data (copy an existing calendar's
# folder there whole, before or after — the service picks it up on restart);
# the binary into /usr/local/bin; the unit into /etc/systemd/system; enable
# and start; then the phone link, read back from the running setup.
#
# It does not install Rust, build, or touch Tailscale — SERVER.md §2–3 —
# and it never deletes or overwrites data: an existing taskdeck_data/ is
# left exactly as it is.

set -eu

USER_NAME=taskdeck
HOME_DIR=/var/lib/taskdeck
DATA_DIR="$HOME_DIR/taskdeck_data"
BIN_DIR=/usr/local/bin
UNIT_DIR=/etc/systemd/system
UNIT=taskdeck-server.service

# The repository root is where this script lives, one level up.
HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(dirname "$HERE")
BINARY="$REPO/target/release/taskdeck-server"

say() { printf '%s\n' "$*"; }
die() { printf 'install.sh: %s\n' "$*" >&2; exit 1; }

[ "$(id -u)" -eq 0 ] || die "run as root: sudo $0"
command -v systemctl >/dev/null 2>&1 || die "systemd is needed (no systemctl on this machine)"
[ -x "$BINARY" ] || die "no built server at $BINARY — run: cargo build --release --bin taskdeck-server"
[ -f "$HERE/$UNIT" ] || die "the unit file $HERE/$UNIT is missing"

# 1. The user. A system account with no shell; its home is where the data goes.
if id "$USER_NAME" >/dev/null 2>&1; then
    say "user $USER_NAME: exists"
else
    say "user $USER_NAME: creating"
    useradd --system --home "$HOME_DIR" --create-home --shell /usr/sbin/nologin "$USER_NAME"
fi

# 2. The folders. Existing data is never touched — only ownership is set,
#    so a folder copied here as root becomes readable by the service.
if [ -d "$DATA_DIR" ]; then
    say "data $DATA_DIR: exists ($(find "$DATA_DIR" -maxdepth 1 -type f | wc -l | tr -d ' ') files)"
else
    say "data $DATA_DIR: creating (empty — copy an existing taskdeck_data/ here to migrate)"
    mkdir -p "$DATA_DIR"
fi
chown -R "$USER_NAME:$USER_NAME" "$HOME_DIR"
chmod 750 "$HOME_DIR"

# 3. The binary. `install` copies atomically enough for a service that is
#    restarted right after; a running server keeps its old inode until then.
say "binary $BIN_DIR/taskdeck-server: installing from $BINARY"
install -m 755 "$BINARY" "$BIN_DIR/taskdeck-server"

# 4. The unit.
say "unit $UNIT_DIR/$UNIT: installing"
install -m 644 "$HERE/$UNIT" "$UNIT_DIR/$UNIT"
systemctl daemon-reload

# 5. Start on boot, and now. A server already running is restarted so it
#    picks up the binary just installed.
if systemctl is-active --quiet "$UNIT"; then
    say "service: running — restarting with the new binary"
    systemctl restart "$UNIT"
else
    say "service: enabling and starting"
    systemctl enable --now "$UNIT"
fi

# Give it a moment to mint a key on first start, then read the link back
# the way SERVER.md §5 says to — without touching the lock or the port.
sleep 2
if systemctl is-active --quiet "$UNIT"; then
    say ""
    say "taskdeck-server is running. The phone link (the key is in it — share it like a password):"
    say ""
    # Captured first: under `set -e` a pipeline's status is its last command's,
    # and a silent failure here would print an empty link and exit 0.
    if links=$(su -s /bin/sh -c "TASKDECK_HOME='$HOME_DIR' '$BIN_DIR/taskdeck-server' --print-link" "$USER_NAME"); then
        printf '%s\n' "$links" | sed 's/^/    /'
    else
        say "    (could not read the link as $USER_NAME; try: journalctl -u taskdeck-server -n 5)"
    fi
    say ""
    say "Logs:  journalctl -u taskdeck-server -f"
else
    say ""
    say "The service did not stay up. Its log:"
    journalctl -u "$UNIT" --no-pager -n 20 || true
    exit 1
fi
