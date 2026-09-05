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
    sync::{Arc, atomic::Ordering, mpsc::{RecvTimeoutError, channel}},
    time::Duration,
};

use chrono::Local;
use task_deck::{
    board::Board,
    color,
    initialization::{self, get_check_and_set_config},
    paths::{self, AppDirs},
    phone::{self, PhoneServer, Pulse},
    subscriptions,
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
--print-link shows the phone links, the data directory and the time zone, then
exits, and draws the first link as a QR code when a terminal is watching — for
checking a setup, or for a link when the service is already running; it reads
the settings and writes nothing. --version prints the version and build date.
";

/// What the command line asked for.
#[derive(Debug, Default, PartialEq)]
struct Options {
    port: Option<u16>,
    bind: Option<String>,
    print_link: bool,
    /// `--help` or `--version`: print it and leave, whatever else was said.
    show: Option<Show>,
}

#[derive(Debug, PartialEq)]
enum Show {
    Help,
    Version,
}

/// Read the arguments — as OS strings, so one that is not text is refused
/// like any other unknown argument rather than panicking the way `env::args`
/// does; the service is started by systemd and by scripts, which can hand it
/// anything. The first thing wrong is the answer, with the flag named.
fn parse_args(args: impl IntoIterator<Item = std::ffi::OsString>) -> Result<Options, String> {
    let mut options = Options::default();
    let mut args = args.into_iter().map(|arg| arg.to_string_lossy().into_owned());
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                options.show = Some(Show::Help);
                return Ok(options);
            }
            "--version" | "-V" => {
                options.show = Some(Show::Version);
                return Ok(options);
            }
            "--print-link" => options.print_link = true,
            "--bind" => {
                let value = args.next().unwrap_or_default();
                match value.trim().parse::<std::net::IpAddr>() {
                    Ok(ip) => options.bind = Some(ip.to_string()),
                    Err(_) => return Err(format!("--bind needs an address such as 0.0.0.0 or 100.64.0.1, not `{value}`")),
                }
            }
            "--port" => {
                let value = args.next().unwrap_or_default();
                match value.parse::<u16>() {
                    Ok(port) if port >= phone::PORT_MIN => options.port = Some(port),
                    _ => return Err(format!("--port needs a number of at least {}, not `{value}`", phone::PORT_MIN)),
                }
            }
            other => return Err(format!("unknown argument `{other}`\n\n{USAGE}")),
        }
    }
    Ok(options)
}

/// Whether to draw the QR at all.
///
/// A terminal on the other end, and nobody having asked for plain text. Piped
/// output stays a link and nothing else, so `--print-link | awk` keeps working
/// and the journal does not fill with block characters every time systemd
/// restarts the unit. `NO_COLOR` skips it rather than printing it uncoloured:
/// without the explicit colours the polarity is the terminal theme's guess,
/// and half the time that is an inverted code no phone will read.
fn show_qr() -> bool {
    use std::io::IsTerminal;
    std::env::var_os("NO_COLOR").is_none() && std::io::stdout().is_terminal() && std::io::stderr().is_terminal()
}

fn main() {
    let options = match parse_args(std::env::args_os().skip(1)) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("{message}");
            exit(2);
        }
    };
    match options.show {
        Some(Show::Help) => {
            print!("{USAGE}");
            return;
        }
        Some(Show::Version) => {
            println!("taskdeck-server {} (built {})", env!("CARGO_PKG_VERSION"), env!("BUILD_DATE"));
            return;
        }
        None => {}
    }
    let Options { port: port_override, bind: bind_override, print_link, .. } = options;

    // A look leaves nothing behind; a run makes its folders.
    let dirs = if print_link { AppDirs::locate() } else { AppDirs::resolve() };

    if print_link {
        // No lock, no bind, no write: this may run beside the service, as
        // another user, to read its link — and must leave nothing behind
        // that the service then cannot write.
        let config = initialization::read_config_only(&dirs.config_file());
        let port = port_override.unwrap_or(config.phone_server_port);
        let bind = bind_override.unwrap_or(config.phone_bind_address);
        println!("data:  {}", dirs.data.display());
        // The runbook in SERVER.md tells people to check this here.
        println!("zone:  {}", Local::now().format("%Y-%m-%d %H:%M %Z (UTC%:z)"));
        if config.phone_token.is_empty() {
            println!("key:   not minted yet — it is, on the first start");
            return;
        }
        for address in phone::addresses_for(&bind) {
            println!("phone: {}", phone::page_url(&address, port, &config.phone_token));
            println!("feed:  {}", phone::feed_url(&address, port, &config.phone_token));
        }
        // And the first of those links as something a phone camera can take
        // straight off the screen, since the alternative is typing a
        // thirty-two character token. Only when a person is watching: this
        // path is piped by `install.sh` and by anyone scripting it, and a
        // screenful of escape codes in the middle of that is not a link.
        if show_qr()
            && let Some(address) = phone::addresses_for(&bind).first()
            && let Some(code) = phone::qr_text(&phone::page_url(address, port, &config.phone_token))
        {
            println!();
            print!("{code}");
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

    // This process owns the board, so this process reads the subscribed
    // calendars (§23). A desktop that is a client of this server does not, and
    // gets the overlay with the board it is a replica of.
    let calendars =
        subscriptions::start(board.subscriptions().to_vec(), board.overlay().clone(), Arc::new(|| {}));

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
    // The zone the phone's day is drawn in. Said out loud because getting it
    // wrong is the quiet failure: everything works, both desktops look right,
    // and only the phone's dates are off at the edges of the day (SERVER.md §4).
    eprintln!("  local time:  {}", Local::now().format("%Y-%m-%d %H:%M %Z (UTC%:z)"));
    for address in phone::addresses_for(&bind) {
        eprintln!("  phone link:  {}", phone::page_url(&address, port, &config.phone_token));
    }
    eprintln!("  feed:        /calendar.ics?token=…   (same host and port)");
    // Started by hand in a terminal, with the phone in the other hand: draw
    // the link so the camera can take it. Under systemd this is a journal
    // socket rather than a terminal, so the journal stays readable — which
    // matters because the unit restarts every three seconds while a bound
    // address has not come up (SERVER.md §5).
    if show_qr()
        && let Some(address) = phone::addresses_for(&bind).first()
        && let Some(code) = phone::qr_text(&phone::page_url(address, port, &config.phone_token))
    {
        eprintln!();
        eprint!("{code}");
    }

    // The whole program: take a request, answer it, publish if it changed
    // anything. The saves inside `Board::apply` are atomic, so a SIGTERM from
    // the service manager at any moment leaves the files whole.
    let mut seen_calendars = 0;
    loop {
        // A timeout rather than a plain `recv`, so a calendar that came back
        // while nobody was asking anything still reaches a parked phone. Two
        // seconds is far below the fetch interval and costs one wakeup.
        match rx.recv_timeout(Duration::from_secs(2)) {
            Ok(request) => {
                let now = Local::now();
                let reply = if request.command.is_query() {
                    phone::answer_query(&mut board, palette, None, &request.command, now)
                } else {
                    match board.apply(request.command, now) {
                        Ok(reply) => {
                            // A change to the list is a change to what gets
                            // fetched, and it is fetched now rather than at
                            // the next tick.
                            pulse.publish(board.version());
                            Ok(phone::reply_json(&reply, board.version()))
                        }
                        Err(error) => Err(error),
                    }
                };
                let _ = request.reply.send(reply);
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }

        // Whatever the calendars said, if they have said anything new. The
        // version moves only when the events actually differ, so a server
        // answering the same thing every ten minutes wakes nobody.
        let version = calendars.version.load(Ordering::Relaxed);
        if version != seen_calendars {
            seen_calendars = version;
            if board.adopt_overlay(calendars.overlay()) {
                board.save_overlay();
                pulse.publish(board.version());
            }
            // A calendar nobody has named takes the best name going: the one
            // the feed gives itself, or failing that its host.
            let renames: Vec<(u64, String)> = board
                .subscriptions()
                .iter()
                .filter_map(|s| subscriptions::better_name(s, board.overlay()).map(|name| (s.id, name)))
                .collect();
            for (id, name) in renames {
                if board.apply(task_deck::board::Command::RenameSubscription { id, name }, Local::now()).is_ok() {
                    pulse.publish(board.version());
                }
            }
            calendars.set_subscriptions(board.subscriptions().to_vec());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn parse(args: &[&str]) -> Result<Options, String> {
        parse_args(args.iter().map(OsString::from))
    }

    #[test]
    fn the_flags_are_read_and_the_last_of_a_repeated_one_wins() {
        let options = parse(&["--port", "7391", "--bind", " 127.0.0.1 ", "--print-link", "--port", "7392"]).unwrap();
        assert_eq!(options, Options { port: Some(7392), bind: Some("127.0.0.1".into()), print_link: true, show: None });
        // Help and version answer at once, before anything after them is judged.
        assert_eq!(parse(&["--help", "--frob"]).unwrap().show, Some(Show::Help));
        assert_eq!(parse(&["-V"]).unwrap().show, Some(Show::Version));
    }

    #[test]
    fn a_bad_or_missing_value_is_refused_with_the_flag_named() {
        for bad in [&["--port", "abc"][..], &["--port", "0"], &["--port", "70000"], &["--port", "1023"], &["--port"]] {
            assert!(parse(bad).unwrap_err().starts_with("--port needs"), "{bad:?}");
        }
        for bad in [&["--bind", "kitchen"][..], &["--bind", ""], &["--bind"]] {
            assert!(parse(bad).unwrap_err().starts_with("--bind needs"), "{bad:?}");
        }
        assert!(parse(&["--frob"]).unwrap_err().starts_with("unknown argument `--frob`"));
    }

    #[cfg(unix)]
    #[test]
    fn an_argument_that_is_not_text_is_unknown_not_a_crash() {
        // `env::args()` panics on one of these before `main` sees it.
        use std::os::unix::ffi::OsStringExt;
        let raw = OsString::from_vec(vec![0xff, 0xfe]);
        let error = parse_args([raw]).unwrap_err();
        assert!(error.starts_with("unknown argument"), "{error}");
    }
}
