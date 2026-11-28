//! Every fuzz target's body over fixed inputs (task S7): the empty input,
//! runs of one byte, every byte value, and pseudo-random inputs of growing
//! length. A target returns on each: none panics, and its fixture builds.

/// The inputs, the same for every target.
fn inputs() -> Vec<Vec<u8>> {
    let mut v = vec![vec![], vec![0], vec![1], vec![0xFF; 8], (0..=255u8).collect(), (0..=255u8).rev().collect()];
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    for n in [1usize, 2, 3, 4, 5, 8, 11, 13, 16, 21, 34, 40, 55, 64, 89, 144, 233, 377, 610] {
        for _ in 0..8 {
            let mut b = Vec::with_capacity(n);
            for _ in 0..n {
                // xorshift64
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                b.push((x >> 24) as u8);
            }
            v.push(b);
        }
    }
    v
}

macro_rules! targets {
    ($($t:ident => $name:ident),* $(,)?) => {
        $(
            #[test]
            fn $name() {
                for (n, input) in inputs().iter().enumerate() {
                    let r = std::panic::catch_unwind(|| crate::$t::run(input));
                    assert!(r.is_ok(), "fuzz target {} panicked on input {n} ({} bytes)", stringify!($name), input.len());
                }
            }
        )*
    };
}

// One test per fuzz target (one per row of the parser table of
// `tests/parsers/main.rs`): its body, fixture included, never panics on
// these inputs.
// spec: SEAL-GEN-011
targets! {
    t_backup_payload => backup_payload,
    t_check_move_code => check_move_code,
    t_classify => classify,
    t_classify_skip_targets => classify_skip_targets,
    t_decode_typed => decode_typed,
    t_file_resume => file_resume,
    t_frame_header => frame_header,
    t_from_accepted => from_accepted,
    t_from_identity_bytes => from_identity_bytes,
    t_from_sheet => from_sheet,
    t_history_record => history_record,
    t_membership => membership,
    t_open_blob => open_blob,
    t_open_file => open_file,
    t_open_snapshot => open_snapshot,
    t_open_snapshot_declared => open_snapshot_declared,
    t_pair_bundle => pair_bundle,
    t_pair_existing => pair_existing,
    t_pair_new => pair_new,
    t_parse_code => parse_code,
    t_payloads => payloads,
    t_read_backup => read_backup,
    t_relay_origin => relay_origin,
    t_snapshot_body => snapshot_body,
    t_unwrap_root => unwrap_root,
    t_verify_backup_file => verify_backup_file,
}
