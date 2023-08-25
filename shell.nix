{ pkgs ? import <nixpkgs> {} }:

pkgs.mkShell {
  buildInputs = with pkgs; [
    cargo
    cmake
    rustc
    rust-analyzer
    bacon
    clippy
    libheif
    openssl
    pkg-config
  ];
}
