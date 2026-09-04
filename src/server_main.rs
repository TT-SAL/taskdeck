//! `taskdeck-server`: the board with no window.
//!
//! The same data, the same commands and the same phone page as the desktop
//! app, served from a machine that is always on and may have no screen at
//! all. It owns `taskdeck_data/`, answers the phone (and, later, desktop
//! clients) over HTTP, and does nothing else — no GPU, no egui, no weather.
//!
//! Where the data lives is decided exactly as it is for the desktop
//! (`paths::AppDirs::resolve`): `$TASKDECK_HOME` if set, else next to the
//! executable, else the per-user data directory. Set `TASKDECK_HOME` in the
//! service unit and the folder is wherever you want it. See `SERVER.md`.

use std::{
    process::exit,
    sync::{Arc, mpsc::channel},
};

use chrono::Local;
use task_deck::{
    board::Board,
    color,
    initialization::{self, get_check_and_set_config},
    paths::{self, AppDirs},
    phone::{self, PhoneServer, Pulse},
};

const USAGE: &str = "\
taskdeck-server — TaskDeck's board, served without a window.

USAGE
    taskdeck-server [--port N] [--bind ADDR] [--print-link] [--version]

The data directory is resolved like the desktop app's: $TASKDECK_HOME if set,
otherwise next to the executable, otherwise the per-user data directory.
`phone_server_port`, `phone_bind_address` and `phone_token` are read from
userconfig.toml there; --port and --bind override the first two for this run
(--bind 100.x.y.z serves the tailnet alone; the default 0.0.0.0 is every
interface). The token is minted on first start.
--print-link shows the phone links and the data directory, then exits — for
checking a setup, or for a link when the service is already running; it reads
the settings and writes nothing. --version prints the version and build date.
";

fn main() {
    let mut port_override: Option<u16> = None;
    let mut bind_override: Option<String> = None;
    let mut print_link = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                print!("{USAGE}");
                return;
            }
            "--print-link" => print_link = true,
            "--bind" => {
                let value = args.next().unwrap_or_default();
                match value.trim().parse::<std::net::IpAddr>() {
                    Ok(ip) => bind_override = Some(ip.to_string()),
                    _ => {
                        eprintln!("--bind needs an address such as 0.0.0.0 or 100.64.0.1, not `{value}`");
                        exit(2);
                    }
                }
            }
            "--version" | "-V" => {
                println!("taskdeck-server {} (built {})", env!("CARGO_PKG_VERSION"), env!("BUILD_DATE"));
                return;
            }
            "--port" => {
                let value = args.next().unwrap_or_default();
                match value.parse::<u16>() {
                    Ok(port) if port >= phone::PORT_MIN => port_override = Some(port),
                    _ => {
                        eprintln!("--port needs a number of at least {}, not `{value}`", phone::PORT_MIN);
                        exit(2);
                    }
                }
            }
            other => {
                eprintln!("unknown argument `{other}`\n\n{USAGE}");
                exit(2);
            }
        }
    }

    let dirs = AppDirs::resolve();

    if print_link {
        // No lock, no bind, no write: this may run beside the service, as
        // another user, to read its link — and must leave nothing behind
        // that the service then cannot write.
        let config = initialization::read_config_only(&dirs.config_file());
        let port = port_override.unwrap_or(config.phone_server_port);
        let bind = bind_override.unwrap_or(config.phone_bind_address);
        println!("data:  {}", dirs.data.display());
        if config.phone_token.is_empty() {
            println!("key:   not minted yet — it is, on the first start");
            return;
        }
        for address in phone::addresses_for(&bind) {
            println!("phone: {}", phone::page_url(&address, port, &config.phone_token));
            println!("feed:  {}", phone::feed_url(&address, port, &config.phone_token));
        }
        return;
    }

    let claim = paths::claim_data_dir(&dirs.data);
    if claim.is_contended() {
        // Two writers clobber each other (DOCUMENTATION.md §4.1). The desktop
        // warns and carries on because a false positive there means "cannot
        // open my own calendar"; a server has no such excuse and stops.
        eprintln!("{}", claim.warning());
        eprintln!("taskdeck-server: another TaskDeck has this data directory open; refusing to start.");
        exit(1);
    }

    // Files that exist but cannot be read are neither quarantined nor
    // started around: a folder copied in as root is the usual cause, and an
    // empty board served in their place would be written over the real one.
    let unreadable = Board::unreadable_files(&dirs.data);
    if !unreadable.is_empty() {
        eprintln!("taskdeck-server: these files exist but cannot be read: {}", unreadable.join(", "));
        eprintln!(
            "taskdeck-server: fix their owner or mode — on a service box, `sudo chown -R taskdeck:taskdeck {}` — and start again; nothing was changed.",
            dirs.data.parent().unwrap_or(&dirs.data).display()
        );
        exit(1);
    }

    let mut config = get_check_and_set_config(&dirs.config_file());
    if config.phone_token.is_empty() {
        config.phone_token = phone::generate_token();
        if let Err(error) =
            initialization::write_config_value(&dirs.config_file(), "phone_token", config.phone_token.clone())
        {
            eprintln!("taskdeck-server: could not save the key to {}: {error}", dirs.config_file().display());
            exit(1);
        }
    }
    let port = port_override.unwrap_or(config.phone_server_port);
    let bind = bind_override.unwrap_or_else(|| config.phone_bind_address.clone());

    let (mut board, problems) = Board::open(dirs.data.clone());
    for problem in &problems {
        eprintln!("taskdeck-server: {problem}");
    }
    // The version counter starts at the clock, not at zero, so a restart
    // never answers a client with a number it has already seen (sync.rs).
    board.seed_version(chrono::Utc::now().timestamp_millis().max(0) as u64);
    // A folder that was a client's replica may hold items under temporary
    // ids; a server must never number in that range.
    match board.adopt_temporaries() {
        Ok(0) => {}
        Ok(n) => eprintln!("taskdeck-server: {n} item(s) carried a client's temporary id and were renumbered."),
        Err(error) => {
            eprintln!("taskdeck-server: {}", error.message);
            exit(1);
        }
    }

    // The phone paints in a colour scheme; the server has no screen, but it
    // has the same file the desktop keeps them in, and the same setting.
    let palette = color::read_colorschemes(&dirs.data)
        .ok()
        .and_then(|schemes| schemes.get(&config.selected_colorscheme_id).map(|scheme| scheme.colors))
        .unwrap_or([[0; 4]; 6]);

    let (tx, rx) = channel();
    let pulse = Arc::new(Pulse::new());
    // Nothing to wake: this thread is only ever waiting on the queue.
    let wake: phone::Wake = Arc::new(|| {});
    let _server = match PhoneServer::start(&bind, port, config.phone_token.clone(), tx, wake, Arc::clone(&pulse)) {
        Ok(server) => server,
        Err(error) => {
            eprintln!("taskdeck-server: {error}");
            exit(1);
        }
    };

    eprintln!(
        "taskdeck-server: serving {} on {}:{port} ({} items)",
        dirs.data.display(),
        phone::host_for_url(&bind),
        board.items.len()
    );
    for address in phone::addresses_for(&bind) {
        eprintln!("  phone link:  {}", phone::page_url(&address, port, &config.phone_token));
    }
    eprintln!("  feed:        /calendar.ics?token=…   (same host and port)");

    // The whole program: take a request, answer it, publish if it changed
    // anything. The saves inside `Board::apply` are atomic, so a SIGTERM from
    // the service manager at any moment leaves the files whole.
    while let Ok(request) = rx.recv() {
        let now = Local::now();
        let reply = if request.command.is_query() {
            phone::answer_query(&mut board, palette, None, &request.command, now)
        } else {
            match board.apply(request.command, now) {
                Ok(reply) => {
                    pulse.publish(board.version());
                    Ok(phone::reply_json(&reply, board.version()))
                }
                Err(error) => Err(error),
            }
        };
        let _ = request.reply.send(reply);
    }
}
