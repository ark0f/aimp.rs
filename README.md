# aimp.rs ![](https://github.com/ark0f/async-socks5/workflows/CI/badge.svg)

AIMP SDK for Rust

# How to use

## Cargo
In `Cargo.toml`:
```toml
[lib]
crate-type = ["cdylib"] # to compile into DLL

[dependencies]
aimp = { git = "https://github.com/ark0f/aimp.rs" }
```

## cargo-aimp
Then you need to install `cargo-aimp` utility:
```
cargo install --git https://github.com/ark0f/aimp.rs --bin cargo-aimp
```
And simply run it:
```
cargo aimp
```
It will build and install plugin, run AIMP with attached console

For more information about cargo-aimp run it with `--help` flag

## Plugin structure
See [examples](examples) and
* [aimp-openmpt](https://github.com/ark0f/aimp-openmpt)

# License
aimp.rs under either of:

* [Apache License 2.0](LICENSE-APACHE.md)
* [MIT](LICENSE-MIT.md)

at your option.
