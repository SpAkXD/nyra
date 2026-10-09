//! Counts the heap memory a thread uses, so the sandboxed interpreter (`ir::interp`) can enforce a
//! memory limit that does not depend on the operating system.
//!
//! The program's global allocator wraps the system one and adds the size of every allocation to a
//! counter of the thread that made it (and subtracts it when the block is freed). The counters
//! are thread-local `Cell`s with no destructor, so the allocator never allocates or fails on its
//! own account. A value that one thread allocates and another frees skews both counters, which
//! does not matter here: the interpreter measures the growth of its own thread between two points.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! {
    static LIVE: Cell<i64> = const { Cell::new(0) };
    static ALLOCS: Cell<u64> = const { Cell::new(0) };
}

/// Bytes allocated by this thread minus bytes it freed.
pub fn live() -> i64 {
    LIVE.with(|c| c.get())
}

/// Allocations made by this thread so far.
pub fn allocs() -> u64 {
    ALLOCS.with(|c| c.get())
}

pub struct Counting;

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn add(bytes: usize) {
    LIVE.with(|c| c.set(c.get().wrapping_add(bytes as i64)));
}

fn sub(bytes: usize) {
    LIVE.with(|c| c.set(c.get().wrapping_sub(bytes as i64)));
}

// SAFETY: every method forwards to the system allocator with the same arguments; the counters
// are only bookkeeping.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = System.alloc(layout);
        if !p.is_null() {
            add(layout.size());
            ALLOCS.with(|c| c.set(c.get().wrapping_add(1)));
        }
        p
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let p = System.alloc_zeroed(layout);
        if !p.is_null() {
            add(layout.size());
            ALLOCS.with(|c| c.set(c.get().wrapping_add(1)));
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
        sub(layout.size());
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let p = System.realloc(ptr, layout, new_size);
        if !p.is_null() {
            sub(layout.size());
            add(new_size);
            ALLOCS.with(|c| c.set(c.get().wrapping_add(1)));
        }
        p
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_vector_counts_while_it_lives() {
        let before = live();
        let v = vec![0u8; 1 << 20];
        assert!(live() - before >= 1 << 20);
        drop(v);
        assert!(live() - before < 1 << 16);
    }
}
