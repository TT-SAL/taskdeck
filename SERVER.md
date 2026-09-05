# Running TaskDeck's board on a server

`taskdeck-server` is TaskDeck's board with no window: the same data files, the same commands,
the same phone page and calendar feed, served from a machine that is always on and may have no
screen at all. Put it on a spare desktop in a cupboard, a mini PC or a Raspberry Pi, reach it over
Tailscale, and the phone view works whether or not your main computer is on. The desktop app can
then be pointed at it (see *The desktop as a client* below) so the board lives in one place.

This document is the setup, start to finish, for a headless Linux box. The reasoning behind the
design is in [`DOCUMENTATION.md` §21–22](DOCUMENTATION.md).

## On the phone

Any current browser runs the page; three settings are worth knowing.

**Turn DNS-over-HTTPS off** for the browser you use. A `*.ts.net` name only resolves through
Tailscale's own resolver — a public one answers `NXDOMAIN` — so a browser that sends its lookups
to Cloudflare or NextDNS cannot find the server at all, and the failure looks like the server being
down rather than like a DNS setting. In Firefox it is Settings → Privacy and security → DNS over
HTTPS → Off. Chrome's default ("automatic") uses the system resolver and is already fine.

**Exempt the browser *and* the Tailscale app from battery optimisation.** On a Samsung, One UI's
sleeping-apps list is the thing that will quietly break this: it puts unused apps to sleep after a
few days, and when it sleeps Tailscale the whole tailnet route goes with it, not just the tab.

**Do not use a private tab.** Service workers in private browsing are recent, and without one there
is no offline shell.

On which browser: on an older Android, prefer whichever still gets updates for it. Chrome's minimum
is currently Android 10 and rises every year or so; Firefox's is Android 8. On a phone that has
stopped receiving Android updates, that difference decides how long the page keeps getting a
patched engine, and it matters more than any rendering difference between them — the page needs
nothing newer than 2020 from any of them.

## First run, in order

Every instruction below is somewhere in this document; what is easy to miss is the sequence, and
the sequence is where a calendar gets lost. Do it in this order.

1. **Build and install the binary** (§3). Nothing is serving yet.
2. **Copy one machine's `taskdeck_data/` up first**, into an empty `/var/lib/taskdeck/taskdeck_data/`,
   and `chown -R taskdeck:taskdeck` it (§4). Pick *one* board: two folders cannot be merged by
   copying, and whichever you do not pick is re-typed by hand.
3. **Set the time zone, before the first start** (§4). The server draws the phone's day in its own
   zone, and an installer's default is usually UTC.
4. **Start the service** (§5).
5. **Check it**: `sudo -u taskdeck TASKDECK_HOME=/var/lib/taskdeck taskdeck-server --print-link`.
   It names the data directory, the zone and the links. Compare the item count in the journal
   against what you copied up.
6. **Point the phone at the server's link** (§5) and delete the old home-screen icon. The phone
   belongs on the machine that is always on, not on a desktop.
7. **Convert the desktops one at a time** (§6): set `server_url` and `server_token`, restart, and
   confirm the board that comes back is the one you copied up — *then* do the next one.
8. **Turn each converted desktop's own phone view off** (Settings → Phone), so there is one link
   to remember rather than one per machine.

Step 2 has to come before step 7. A desktop's first successful connection to a server sets its own
board aside — harmless once the server already holds the right board, and the moment a calendar is
lost if it does not.

## Trying it on a Mac first

The whole of this document is about a Linux box, but the server is a plain binary and runs on the
machine you already have. That is the cheapest way to find out whether you want the box at all.

```sh
cargo build --release
./target/release/taskdeck-server        # serves taskdeck_data/ beside the binary
```

Four things bite on macOS and nowhere else:

- **One writer.** The server takes the same lock the desktop app does (§4.1), so TaskDeck refuses
  to start while the server is running on the same folder. Pick one.
- **The firewall keys permission by path.** macOS asks once per binary, and `target/debug/` and
  `target/release/` are two different binaries to it. Allowing the debug build during development
  and then running the release build gives you a server that answers on `127.0.0.1` and is
  silently unreachable from everywhere else. Allow it explicitly rather than waiting for a prompt
  that may not appear:
  ```sh
  sudo /usr/libexec/ApplicationFirewall/socketfilterfw --add "$PWD/target/release/taskdeck-server"
  ```
- **Sleep.** A closed lid is a stopped server. Nothing in the app can prevent that.
- **No launchd unit ships with this.** The server runs until you stop it or reboot.

### Tailscale on a Mac

Install the **standalone** build from `tailscale.com/download/mac`, not the App Store one: the
sandboxed version does not give a usable `tailscale serve`, which is what puts HTTPS on the phone
page. Sign in on the Mac and on the phone with the same account, and `tailscale status` should list
both.

Nothing about the server needs changing: `phone_bind_address` defaults to `0.0.0.0`, which is every
interface, and the tailnet's is one of them. The phone can open
`http://<machine>.<tailnet>.ts.net:7373/?token=…` from mobile data straight away.

**HTTPS is a separate switch**, and worth throwing. It is what lets the browser install the service
worker, which is what makes the page open at all when the phone has no signal (§21.5). Enable
HTTPS certificates for the tailnet once, in the admin console under DNS, then:

```sh
sudo tailscale serve --bg 7373
```

Leave `phone_bind_address` at `0.0.0.0` when you do. `serve` hands requests to `127.0.0.1`, and a
single-address bind refuses them — the same trap §2 records for the Linux side.

## 1. The machine and its OS

Any x86-64 or ARM64 box with a couple of gigabytes of RAM is more than enough; the server idles
doing nothing and wakes for a few milliseconds per request. What matters is power, and the levers
in order of effect:

- **Debian or Ubuntu Server, minimal, no desktop environment.** A headless install idles lowest
  and is the natural home for a service that must start on boot and restart on failure.
- **Remove a discrete graphics card** if the CPU has integrated graphics. The server never touches
  a GPU.
- **An SSD**, not a spinning disk.
- **BIOS:** enable the deep C-states, "ErP"/"EuP" power saving, and *power on after power loss*.
- After installing: `sudo apt install powertop && sudo powertop --auto-tune` once, and again on
  boot via a service if it helps (measure with a plug-in power meter — it is the only real number).

A ten-year-old office desktop treated this way lands around 15–30 W; a mini PC or a Raspberry Pi
around 5–10 W. At €0.10–0.20 per kWh that is €2–5 a month for the desktop and under €1 for the
small boards.

## 2. Tailscale

Tailscale gives the server, your phone and your desktop a private network that works from
anywhere — mobile data, hotel Wi-Fi — through any NAT, with no router configuration and nothing
exposed to the internet. Install it on all three and sign in with the same account:

```bash
curl -fsSL https://tailscale.com/install.sh | sh
sudo tailscale up
```

The server gets a stable `100.x.y.z` address and a name like `spare.tail1234.ts.net`. Do **not**
port-forward TaskDeck instead: it speaks plain HTTP with a key in the URL, which is fine inside a
tailnet and not fine on the open internet. If the box also sits on a LAN you would rather not
serve, bind to the Tailscale address alone — `phone_bind_address = "100.x.y.z"` in
`userconfig.toml`, or `--bind 100.x.y.z` for one run — and the port is not open anywhere else.
(Not together with `tailscale serve` below, which hands requests to `127.0.0.1:7373`: with it,
keep the default bind, or serve the tailnet address explicitly with
`sudo tailscale serve --bg http://100.x.y.z:7373`.)

**Optional: HTTPS on the tailnet name.** Tailscale can front the server with a real certificate,
so the phone opens `https://spare.tail1234.ts.net/?token=…` instead of an IP address:

```bash
sudo tailscale serve --bg 7373
```

Everything works the same over plain HTTP; the difference is that a browser treats the HTTPS page
as a secure context, which is what lets the phone page install its offline shell — with it, the
page opens and shows the last day it saw even when the server is unreachable (edits made then
are kept on the phone and sent when the server answers, over either form). The desktop's
`server_url` can use either form.

## 3. Building

A Rust binary is compiled per OS, so the server needs a build on (or for) the Linux box. On the
box itself:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
git clone <this repository> taskdeck && cd taskdeck
cargo build --release --bin taskdeck-server
```

The result is `target/release/taskdeck-server`, one self-contained executable — no window, and
no graphics library needed to run it. The build itself compiles the whole crate, the desktop's
graphics crates included (they are one crate; nothing in the server calls them and the link drops
them), so expect the first build to take a while and to want the same Rust toolchain the desktop
does, nothing more. A plain `cargo build --release` builds `TaskDeck` too, which the box does not
need.

```bash
sudo install -m 755 target/release/taskdeck-server /usr/local/bin/
```

## 4. The data

The server keeps its data exactly where the desktop app does — a `taskdeck_data/` folder — and
finds it the same way: `$TASKDECK_HOME` if set, else next to the executable, else the per-user
data directory. Set `TASKDECK_HOME` in the service below and the folder lives wherever you want.

To move an existing calendar onto the server, **copy the desktop's `taskdeck_data/` folder there
whole** — `read_at_startup.json`, `archived.jsonl`, `notepad_text.json`, `colorschemes.json` and
`userconfig.toml` — **make the service user its owner** (`sudo chown -R taskdeck:taskdeck
/var/lib/taskdeck`; the desktop writes its files readable by their owner only, and a file the
service cannot read stops it from starting, on purpose, rather than serving an empty board over
it), and from then on run the desktop as a client of the server (§6), not on its own copy. Two
TaskDecks writing the same board overwrite each other (`DOCUMENTATION.md` §4.1);
`taskdeck-server` refuses to start if another TaskDeck has the folder open.

`userconfig.toml` on the server needs only two keys; everything else in it is about a screen the
server does not have and is ignored:

```toml
phone_server_port = 7373
phone_token = "…"              # minted on first start if missing — see the log
phone_bind_address = "0.0.0.0" # every interface; a single address serves that one only (§2)
```

`selected_colorscheme_id` is honoured if present: the phone paints in that scheme.

### The time zone

The server draws the phone's day, the now-line and the planner's figures in **its own** local zone.
A Linux installer's default is usually UTC, and a UTC box serving someone three hours east puts
their evening blocks on the previous day — on the phone only, while both desktops look right, which
makes it the hardest kind of wrong to notice. Set it before the first start:

```sh
sudo timedatectl set-timezone Europe/Helsinki
```

`taskdeck-server --print-link` and the journal both print the zone they will use, so it can be
checked rather than assumed. A box that has to stay on UTC for other reasons can carry
`Environment=TZ=…` in the unit instead — edit `deploy/taskdeck-server.service` in the repository,
not the installed copy, because `install.sh` reinstalls the unit on every run and would put your
edit back.

## 5. The service

Everything in this section is also one command, `sudo deploy/install.sh`, run from the repository
after the build in §3. It creates the user and the folder if they are missing, installs the binary
and the unit, enables and starts the service (or restarts it, on an update), and prints the phone
link. It never touches an existing `taskdeck_data/` beyond setting its owner, so it is safe to
run again. The steps it takes, by hand:

Run it as its own user, started on boot and restarted on failure:

```bash
sudo useradd --system --home /var/lib/taskdeck --create-home --shell /usr/sbin/nologin taskdeck
sudo mkdir -p /var/lib/taskdeck/taskdeck_data
# (copy your taskdeck_data/ contents into /var/lib/taskdeck/taskdeck_data/ here, if migrating)
sudo chown -R taskdeck:taskdeck /var/lib/taskdeck
```

`/etc/systemd/system/taskdeck-server.service` — a copy of this file ships in the repository as
`deploy/taskdeck-server.service`, so `sudo install -m 644 deploy/taskdeck-server.service
/etc/systemd/system/` is enough:

```ini
[Unit]
Description=TaskDeck board server
After=network-online.target tailscaled.service
Wants=network-online.target

[Service]
User=taskdeck
Group=taskdeck
Environment=TASKDECK_HOME=/var/lib/taskdeck
ExecStart=/usr/local/bin/taskdeck-server
Restart=always
RestartSec=3
# Hardening: the server needs only its own folder and the network.
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/var/lib/taskdeck
PrivateTmp=true
PrivateDevices=true
ProtectKernelTunables=true
ProtectControlGroups=true
RestrictSUIDSGID=true

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now taskdeck-server
journalctl -u taskdeck-server -f
```

The log's first lines say where the data is, which port it serves, and the phone link (with the
key) for each address the machine has — the Tailscale one is the one to put on the phone. Open it
there once and add the page to the home screen. The calendar feed is `/calendar.ics?token=…` on
the same host and port. (The key is therefore in the journal, readable by anyone who can read the
journal on that box — the same people who can read `userconfig.toml`. **New key** is a matter of
editing `phone_token` there and restarting the service.)

On a cold boot the Tailscale address can come up after the service starts, in which case the
first log lines show the LAN link only; `--print-link` a minute later shows both, and a service
bound to the Tailscale address alone is simply restarted by systemd every three seconds until the
address exists.

Getting the link onto the phone is the one fiddly part, and the server does it for you: run
`--print-link` **in a terminal** and it draws the first link as a QR code under the text. Point the
phone's camera at it. On a Samsung the scanner is a one-time toggle behind the Camera app's gear
("Scan QR codes"); Google Lens works otherwise. Piping the output suppresses the code and leaves a
plain link, so scripts are unaffected. Never send the link through email or a messenger: it is a
bearer credential, it travels over plain HTTP, and every hop keeps a copy.

`taskdeck-server --help` lists the flags: `--port N` and `--bind ADDR` override the port and
the address for one run;
`--print-link` prints the data directory and the phone and feed links without serving (safe to
run beside the service, e.g. `sudo -u taskdeck TASKDECK_HOME=/var/lib/taskdeck taskdeck-server
--print-link`; it reads the port and the bind from the file, so if the service runs with `--port`
or `--bind` overrides, pass the same ones); `--version` says what is built.

## 6. The desktop as a client

The desktop app can run against the server instead of its own files, so the wall calendar, the
phone and any other computer all look at one board:

```toml
# in the desktop's own userconfig.toml
server_url = "http://100.x.y.z:7373"
server_token = "…"            # the server's phone_token
```

With those set, the desktop loads the board from the server at startup, sends every edit to it as
the same command the phone would send, and refreshes the moment anything changes there. Its own
`taskdeck_data/` becomes a cache of the last board it saw. **Do the copy in §4 first**: on its
first successful connection the desktop sets any board that lived in its folder aside — dated
files like `read_at_startup.json.local-20260904-121500`, never deleted — and tells you; if you
forgot the copy, those files are your calendar — but copying them across restores nothing on its
own: **rename each back to the name before `.local-`**, because the server opens only the plain
names. It is safe only while the server's board is still empty; once it holds one, the two have to
be merged by hand. Restart the service after. **If the server is unreachable** the
desktop keeps working on that cache and queues its edits in `taskdeck_data/outbox.json`; when the
server is back they are replayed in order, the server's picture wins, and anything that could not
be applied (an item finished meanwhile from the phone, say) is reported rather than silently
dropped. Every command travels under a key that is the same on every retry, so a reply lost on
the way — or a server that answered late — never means an edit applied twice. The menu bar says
when edits are waiting, and says `outbox not saved` if the desktop cannot write its own outbox.

**To go back to a local board**, clear `server_url` and restart: the folder is its own board again,
any edits still unsent are set aside as a dated `outbox.json.local-…` (never replayed against a
later server, never deleted), and a later return to the server is a first contact again — the
local board set aside, as above, rather than overwritten.

## 7. Backups

The whole board is a handful of small text files. A nightly copy is enough:

```sh
#!/bin/sh
# /etc/cron.daily/taskdeck-backup  (make it executable: sudo chmod +x)
tar -C /var/lib/taskdeck -czf "/var/backups/taskdeck-$(date +%F).tar.gz" taskdeck_data
find /var/backups -name 'taskdeck-*.tar.gz' -mtime +30 -delete
```

Restoring is copying the folder back, `chown -R taskdeck:taskdeck` on it, and restarting the
service. `archived.jsonl` is append-only and `read_at_startup.json` is written atomically, so each
file in a backup is whole at any moment. The pair can straddle a completion by a few milliseconds
— the archive row appended, the live set not yet rewritten — and a backup taken exactly then
restores that task both live and in the ledger, which is visible at once and mended with one ✓.

## 8. Updating

Pull, rebuild, install, restart:

```bash
cd ~/taskdeck && git pull && cargo build --release --bin taskdeck-server
sudo deploy/install.sh
```

The script notices the service is running and restarts it with the new binary; by hand, that is
`sudo install -m 755 target/release/taskdeck-server /usr/local/bin/ && sudo systemctl restart
taskdeck-server`.

The data format is stable across versions: new fields are `#[serde(default)]`, and older files
load unchanged (`DOCUMENTATION.md` §6). Update the server and the desktop from the same commit
when you can: a newer desktop may send a command an older server does not know, which the
server refuses and the desktop then reports and drops — nothing is corrupted, but that edit is
lost.
