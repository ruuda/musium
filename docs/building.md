# Building

Musium is written in [Rust][rust] and [Purescript][purescript], so you need to
have the build tools for these available. An easy way to get them is through
[the Nix package manager][nix]. The following command enters a shell in which
all of the required build tools are available:

    nix run --command $SHELL

This environment is also tested on <abbr>CI</abbr>. Nix is a convenience, not
a requirement. You are free to source the build tools elsewhere, for example
from your system package repositories.

The library browser is written in [Purescript][purescript]. There is a basic
makefile that calls `purs` and `psc-package`:

    make -C app
    stat app/output/app.js

The server will serve `app.js` and other static files alongside the API. The
server itself is written in [Rust][rust] and builds with Cargo:

    cargo build --release

The binary can then be found in `target/release/musium`.

[nix]:        https://nixos.org/
[rust]:       https://rust-lang.org
[purescript]: http://www.purescript.org/
