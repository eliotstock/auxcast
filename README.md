# Auxcast

This humble tool allows you to take a line input such as the one from your turntables, mixer, CD player or old school amp and play it on your Chromecast speakers.

You'll need an external soundcard that has a line-in on it. You should run this while connected to the same WiFi network as your Chromecast devices.

I'm providing this as source only for now. Raise a bug if you'd like a binary release. It's pure Rust so far, so it should build and run on Mac OS, Windows and Linux.

## Prerequisites

1. Install Rust using [rustup](https://rustup.rs/):
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```

2. After installation, ensure your Rust toolchain is up to date:
   ```bash
   rustup update
   ```

3. Verify the installation:
   ```bash
   rustc --version
   cargo --version
   ```

## Building

To build the project for running on the same machine:

```bash
cargo build
```

To cross compile from MacOS to Windows:

```bash
rustup target add x86_64-pc-windows-gnu
brew install mingw-w64
cargo build --target x86_64-pc-windows-gnu --release
```

The resulting binary will be in `target/x86_64-pc-windows-gnu/release/auxcast.exe`

## Running

Run it with:
```bash
cargo run
```

## Issues

### Known issues:

* Speaker groups are not yet supported. These are going to need me to switch from WAV format to mp3 or AAC and that involves external dependencies.

Please raise a bug if anything else doesn't work for you. I could do with the output when run in verbose mode:

```bash
cargo run -- --verbose
```
