# This sets up a development environment using nix.
# It useful to anyone who uses nix and useless otherwise.

let
  moz_overlay = import (builtins.fetchTarball https://github.com/mozilla/nixpkgs-mozilla/archive/master.tar.gz);
  nixpkgs = import <nixpkgs> { overlays = [ moz_overlay ]; };
  rustChannel = nixpkgs.latest.rustChannels.stable.rust.override { 
    extensions = [
      "rust-analysis"
      "rust-src"
    ];
  };
in
with nixpkgs;
mkShell {
  name = "voice-keyboard-linux";

  buildInputs = [
    alsa-lib.dev
    openssl
    pkg-config
    rust-analyzer
    rustChannel
    gtk3
    gdk-pixbuf
    pango
    cairo
    glib
    libxdo
  ];

  shellHook = ''
    export RUST_BACKTRACE=1;
  '';
}
