# Affine IO

Affine IO is now a Rust-only repository. It builds a single Rust `cdylib` that
exports the current `segatools` ABI for `aimeio`, `mai2io`, `chuniio`, and
`mercuryio`.

## Build

- Toolchain: stable Rust with the MSVC targets
- x64: `cargo build --release`
- x86: `cargo build --release --target i686-pc-windows-msvc`

Output DLLs:

- x64: `target/release/affine_io.dll`
- x86: `target/i686-pc-windows-msvc/release/affine_io.dll`

## Usage

Point the relevant `segatools` DLL paths at the same physical DLL, or copy the
same built binary under the expected file names such as `aimeio.dll`,
`mai2io.dll`, `chuniio.dll`, and `mercuryio.dll`.

The runtime keeps compatibility with `SEGATOOLS_CONFIG_PATH` and
`.\\segatools.ini`. The current hardware target is the Affine serial stack plus
the Monica Sega-serial NFC reader.

## CI

GitHub Actions now includes:

- `CI`: `fmt`, `clippy`, and `cargo test`
- `Build`: release DLL builds for both `x64` and `x86`, with packaged alias
  names for `aimeio.dll`, `mai2io.dll`, `chuniio.dll`, and `mercuryio.dll`

## Commercial use

Please contact the author before any commercial use.

## Community

QQ group: 531883107

## License

Licensed under the Business Source License 1.1. The Change Date is 2029-12-22,
after which the project becomes GPL-3.0-only. See LICENSE.
