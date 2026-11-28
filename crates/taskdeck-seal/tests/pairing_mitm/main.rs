//! Pairing (task S6, seal.md §17): a relay in the middle that knows the code
//! and holds every slot (`s6_mitm`), and both machines through whole
//! pairings against a simulated relay and board, with every check of §17.6
//! and §17.9 failing once and the rotation races (`s6_flow`); slots
//! delivered twice (`s7b_repeats`).

#[path = "../common/mod.rs"]
mod common;
#[path = "../common/board.rs"]
mod board;
#[path = "../common/backup.rs"]
mod backup;
#[path = "../common/blob.rs"]
mod blob;
#[path = "../common/pairing.rs"]
mod pairing;

mod s6_flow;
mod s6_mitm;
mod s7b_repeats;
