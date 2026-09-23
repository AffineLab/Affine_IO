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
- `crates/affine-mercury`: WACCA runtime

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
  Input is never read from the Vendor HID command interface: it carries the
  replies to every host's commands, so only the board-info reply is used there.
- `chuni` / `mercury`: the touch slider runs over USB-CDC serial.
- `aime`: the Monica NFC reader runs over Sega serial.

Each runtime also publishes its state through named shared-memory pages so other
tools can mirror it. `mai2` exposes its input pages under the `mai_io_shm_1` and
`mai_io_shm_2` mappings.

### `segatools.ini` keys

`mai2` reads these (defaults in parentheses):

- `[touch] p1Enable` / `p2Enable` (`1`): enable each player's touch runtime.
- `[touch] p1DebugInput` / `p2DebugInput` (`0`): enable per-player touch
  diagnostic logging.
- `[io4] test` / `service` / `coin`: virtual-key codes for the operator-button
  keyboard fallback (default `VK_F1` / `VK_F2` / `VK_F3`).

### `mai2` function buttons

Each board sends its six function buttons as one byte, one bit per pin:

| Bit | Pin | Button | Game input |
|:--|:--|:--|:--|
| 0 | PC4 | Test | Test, from either board |
| 1 | PB0 | Service | Service, from either board |
| 2 | PB1 | Coin | one credit per press, from either board |
| 3 | PB2 | Card scan | not read by Affine IO: the board's own keyboard sends Enter, which segatools' built-in Aime emulation reads as `[aime] scan` |
| 4 | PB10 | 1P Select | player 1 Select, from either board |
| 5 | PB11 | 2P Select | player 2 Select, from either board |

The pin decides the player, not the board, so a board wired with both Select
buttons gives both players their Select. Wire the 2P board's Select to PB11: on
PB10 it is player 1's Select.

**Upgrading from v1.1.2 or v1.2.0-rc.1:** those versions read this byte one
place off. Test acted as that player's Select, Service acted as Test, Coin acted
as Service (so neither board's Coin gave a credit), the 1P card-scan key acted
as Coin (so a card tap also gave a credit), and both Select buttons did nothing.
This version restores the labels on the buttons. If you relabelled or rewired
buttons to work around the old mapping, undo that. CurvaMods builds made before
its own function-button fix read the byte the same old way and add it to the
game's input, so update CurvaMods together with this version: with an old build,
Test, Service and Coin each also press a second button (Test also presses
Select).

## CI

GitHub Actions includes:

- `CI`: `cargo fmt --check`, release-profile workspace `clippy` (warnings denied),
  and workspace `cargo test`
- `Build`: release DLL builds for both `x64` and `x86`, packaging only
  `affine_io.dll`
- `Release`: on a `v*` tag, builds both targets and publishes a GitHub Release
  with the `x64` and `x86` DLLs attached

## Commercial use

Please contact the author before any commercial use.

## Community

QQ group: 531883107

## License

Licensed under the Business Source License 1.1. The Change Date is 2029-12-22,
after which the project becomes GPL-3.0-only. See LICENSE.
