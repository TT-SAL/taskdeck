//! What the crate leaves in freed heap memory. This target installs a test
//! allocator that, while a test arms it on its own thread, scans every block
//! freed on that thread for byte patterns the test names (the crate itself
//! stays free of `unsafe`; only this test binary's allocator needs it). Each
//! task adds its module here.

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

mod s5_roots;
mod s6_typed;
mod s7_sheet;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::{Cell, RefCell};

thread_local! {
    static ARMED: Cell<bool> = const { Cell::new(false) };
    static PATTERNS: RefCell<Vec<Vec<u8>>> = const { RefCell::new(Vec::new()) };
    /// (size of the freed block, index of the pattern found in it).
    static HITS: RefCell<Vec<(usize, usize)>> = const { RefCell::new(Vec::new()) };
}

struct Spy;

// SAFETY: every call is passed on to `System` unchanged; the scan only reads
// the block being freed, which is still allocated and `l.size()` bytes long.
unsafe impl GlobalAlloc for Spy {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        unsafe { System.alloc(l) }
    }

    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        // Disarmed while scanning, so that recording a hit (which may
        // allocate or free) is not scanned itself.
        if ARMED.try_with(|a| a.replace(false)).unwrap_or(false) {
            let block = unsafe { std::slice::from_raw_parts(p, l.size()) };
            let found: Vec<usize> = PATTERNS.with(|ps| {
                ps.borrow().iter().enumerate().filter(|(_, pat)| block.windows(pat.len()).any(|w| w == &pat[..])).map(|(i, _)| i).collect()
            });
            HITS.with(|h| h.borrow_mut().extend(found.into_iter().map(|i| (l.size(), i))));
            ARMED.with(|a| a.set(true));
        }
        unsafe { System.dealloc(p, l) }
    }
}

#[global_allocator]
static ALLOC: Spy = Spy;

/// Runs `f` with the scan armed on this thread for `patterns`; returns every
/// (block size, pattern index) found in a block freed meanwhile.
pub fn freed_with(mut patterns: Vec<[u8; 32]>, f: impl FnOnce()) -> Vec<(usize, usize)> {
    let any = patterns.iter().map(|p| p.to_vec()).collect();
    // Wiped before their buffer is freed (see `s5_roots::many_roots`).
    patterns.iter_mut().for_each(|x| *x = [0; 32]);
    core::hint::black_box(&mut patterns);
    drop(patterns);
    freed_with_bytes(any, f)
}

/// `freed_with` for patterns of any non-zero length.
pub fn freed_with_bytes(patterns: Vec<Vec<u8>>, f: impl FnOnce()) -> Vec<(usize, usize)> {
    assert!(patterns.iter().all(|p| !p.is_empty()));
    PATTERNS.with(|p| *p.borrow_mut() = patterns);
    HITS.with(|h| h.borrow_mut().clear());
    ARMED.with(|a| a.set(true));
    f();
    ARMED.with(|a| a.set(false));
    // The patterns are wiped before their buffers are freed, for the same
    // reason (see `s5_roots::many_roots`).
    PATTERNS.with(|p| {
        let mut p = p.borrow_mut();
        p.iter_mut().for_each(|x| x.iter_mut().for_each(|b| *b = 0));
        core::hint::black_box(&mut *p);
        p.clear();
    });
    HITS.with(|h| std::mem::take(&mut *h.borrow_mut()))
}
