//! The clocks and the random seed of the interpreter, on every target.
//!
//! Natively they come from the operating system. WebAssembly in a browser has no operating system
//! (`std::time::Instant::now` panics there), so the page gives them as three imported functions
//! of the module `env` (see `src/wasm.rs`):
//!
//! - `nyra_now_ms() -> f64`: milliseconds since 1970 (`Date.now()`);
//! - `nyra_mono_ms() -> f64`: a monotonic clock in milliseconds (`performance.now()`);
//! - `nyra_random() -> f64`: a random number in [0, 1) (`Math.random()`).

#[cfg(target_arch = "wasm32")]
use std::time::Duration;

#[cfg(not(target_arch = "wasm32"))]
pub use std::time::Instant;

/// Milliseconds since 1970.
#[cfg(not(target_arch = "wasm32"))]
pub fn unix_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

/// 32 random bits from the operating system's generator; `n` makes each draw different.
#[cfg(not(target_arch = "wasm32"))]
pub fn os_random(n: u64) -> u32 {
    // the standard library's hasher keys come from the operating system's generator
    use std::hash::{BuildHasher as _, Hasher as _};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u64(n);
    (h.finish() >> 16) as u32
}

#[cfg(target_arch = "wasm32")]
mod host {
    #[link(wasm_import_module = "env")]
    extern "C" {
        pub fn nyra_now_ms() -> f64;
        pub fn nyra_mono_ms() -> f64;
        pub fn nyra_random() -> f64;
    }
}

/// A point on the monotonic clock of the page.
#[cfg(target_arch = "wasm32")]
#[derive(Clone, Copy, Debug)]
pub struct Instant(f64);

#[cfg(target_arch = "wasm32")]
impl Instant {
    pub fn now() -> Instant {
        // SAFETY: an imported function without arguments that returns a number
        Instant(unsafe { host::nyra_mono_ms() })
    }

    pub fn elapsed(&self) -> Duration {
        let ms = (Instant::now().0 - self.0).max(0.0);
        Duration::from_nanos((ms * 1e6) as u64)
    }
}

#[cfg(target_arch = "wasm32")]
pub fn unix_ms() -> i64 {
    // SAFETY: as above
    unsafe { host::nyra_now_ms() as i64 }
}

#[cfg(target_arch = "wasm32")]
pub fn os_random(_n: u64) -> u32 {
    // SAFETY: as above
    let r = unsafe { host::nyra_random() };
    (r.clamp(0.0, 1.0) * 4294967296.0) as u64 as u32
}
