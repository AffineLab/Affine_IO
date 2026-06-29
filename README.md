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

Configuration is read from `SEGATOOLS_CONFIG_PATH` if set, otherwise from
`.\\segatools.ini`. All Affine controllers share USB Vendor ID `VID_AFF1`.

### Transports

- `mai2`: USB-HID is the primary transport; the USB-CDC serial path is kept as a
  fallback. Touch, buttons, and LEDs are carried over whichever link is live.
- `chuni` / `mercury`: the touch slider runs over USB-CDC serial.
- `aime`: the Monica NFC reader runs over Sega serial.

Each runtime also publishes its state through named shared-memory pages so other
tools can mirror it. `mai2` exposes its input pages under the `mai_io_shm_1` and
`mai_io_shm_2` mappings.

### `segatools.ini` keys

`mai2` reads these (defaults in parentheses):

- `[touch] p1Enable` / `p2Enable` (`1`): enable each player's touch runtime.
- `[touch] p1DebugInput` / `p2DebugInput` (`0`): enable per-player touch
  diagnostic logging. (`pxDebugInput` is segatools' own keyboard-touch mapping;
  Affine only reuses the flag to gate its diagnostics.)
- `[io4] test` / `service` / `coin`: virtual-key codes for the operator-button
  keyboard fallback (default `VK_F1` / `VK_F2` / `VK_F3`).

## CI

GitHub Actions includes:

- `CI`: `cargo fmt --check`, release-profile workspace `clippy` (warnings denied),
  and workspace `cargo test`
- `Build`: release DLL builds for both `x64` and `x86`, packaging only
  `affine_io.dll`
- `Release`: on a `v*` tag, builds both targets and publishes a GitHub Release
  with the `x64` and `x86` DLLs attached

## Latency benchmark

A Windows-only end-to-end latency benchmark is available as an example binary.
By default it talks to benchmark-enabled Affine STM32 firmware over serial and
measures:

- host-observed round-trip time
- device-side handling time from `t_rx` to `t_tx`
- host-minus-device time as a quick transport/stack estimate
- estimated `host -> device` one-way latency
- estimated `device -> host` one-way latency
- calibrated `CDC event -> host` one-way latency on `mai2`
- calibrated `HID event -> host` one-way latency on `mai2`

- Run: `cargo run --example e2e_latency_bench --features latency-bench`
- Iterations: `cargo run --example e2e_latency_bench --features latency-bench -- --iterations=1000`
- Synthetic only: append `--synthetic`
- Hardware plus synthetic: append `--all`

The hardware mode currently expects benchmark firmware support:

- `0x22` for round-trip timing
- `0x23` for CDC event timing
- `0x24` for HID event timing on `Mai_stm32`

`--synthetic` skips hardware probing and only runs the old in-process
callback/poll measurements for `mai2`, `chuni`, and `mercury`.

The one-way figures are estimates derived from host send/receive timestamps plus
firmware `t_rx`/`t_tx` timestamps. They are useful for directionality, but they
are not a strict clock-synchronized ground truth.

`Mai_stm32` benchmark firmware can now also emit a scheduled `0x23` event packet.
When that firmware is present, the benchmark prints:

- calibrated `event -> host` latency
- calibrated `tx -> host` latency

These values use the `0x22` RTT samples to estimate a host/device clock offset,
then measure the host receive time of the later device-originated event packet.
They are direction-specific and more representative of `STM32 -> host` event
delivery than simply splitting the RTT in half.

## Commercial use

Please contact the author before any commercial use.

## Community

QQ group: 531883107

## License

Licensed under the Business Source License 1.1. The Change Date is 2029-12-22,
after which the project becomes GPL-3.0-only. See LICENSE.
