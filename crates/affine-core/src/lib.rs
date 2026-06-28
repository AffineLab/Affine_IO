pub mod serial;
pub mod shared_memory;
pub mod slider;
pub mod types;
pub mod util;

/// Build version stamped by `build.rs`: `git describe --tags --always --dirty`,
/// falling back to the Cargo package version when git is unavailable.
pub fn version() -> &'static str {
    env!("AFFINE_BUILD_VERSION")
}
