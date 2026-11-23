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

mod s5_roots;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::{Cell, RefCell};

thread_local! {
    static ARMED: Cell<bool> = const { Cell::new(false) };
    static PATTERNS: RefCell<Vec<[u8; 32]>> = const { RefCell::new(Vec::new()) };
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
                ps.borrow().iter().enumerate().filter(|(_, pat)| block.windows(32).any(|w| w == &pat[..])).map(|(i, _)| i).collect()
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
pub fn freed_with(patterns: Vec<[u8; 32]>, f: impl FnOnce()) -> Vec<(usize, usize)> {
    PATTERNS.with(|p| *p.borrow_mut() = patterns);
    HITS.with(|h| h.borrow_mut().clear());
    ARMED.with(|a| a.set(true));
    f();
    ARMED.with(|a| a.set(false));
    // The patterns are wiped before their buffer is freed, for the same
    // reason (see `s5_roots::many_roots`).
    PATTERNS.with(|p| {
        let mut p = p.borrow_mut();
        p.iter_mut().for_each(|x| *x = [0; 32]);
        core::hint::black_box(&mut *p);
        p.clear();
    });
    HITS.with(|h| std::mem::take(&mut *h.borrow_mut()))
}
