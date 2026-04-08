# Affine IO

Affine IO is now a Rust-only workspace. It builds a single Rust `cdylib` that
exports the current `segatools` ABI for `aimeio`, `mai2io`, `chuniio`, and
`mercuryio`.

## Workspace

- Root crate `affine-io`: the single exported `cdylib`
- `crates/affine-core`: shared Win32, serial, config, and protocol helpers
- `crates/affine-aime`: Aime/NFC runtime
- `crates/affine-mai2`: maimai runtime
- `crates/affine-chuni`: CHUNITHM runtime
- `crates/affine-mercury`: Project DIVA/Mercury runtime

## Build

- Toolchain: stable Rust with the MSVC targets
- x64: `cargo build -p affine-io --release`
- x86: `cargo build -p affine-io --release --target i686-pc-windows-msvc`

Output DLLs:

- x64: `target/release/affine_io.dll`
- x86: `target/i686-pc-windows-msvc/release/affine_io.dll`

## Usage

Point the relevant `segatools` DLL path at the single built `affine_io.dll`.

The runtime keeps compatibility with `SEGATOOLS_CONFIG_PATH` and
`.\\segatools.ini`. The current hardware target is the Affine serial stack plus
the Monica Sega-serial NFC reader.

## CI

GitHub Actions now includes:

- `CI`: `fmt`, workspace `clippy`, and workspace `cargo test`
- `Build`: release DLL builds for both `x64` and `x86`, packaging only
  `affine_io.dll`

## Commercial use

Please contact the author before any commercial use.

## Community

QQ group: 531883107

## License

Licensed under the Business Source License 1.1. The Change Date is 2029-12-22,
after which the project becomes GPL-3.0-only. See LICENSE.
