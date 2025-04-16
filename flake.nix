{
    inputs = {
        nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
        flake-utils.url = "github:numtide/flake-utils";
        rust-overlay = {
            url = "github:oxalica/rust-overlay";
            inputs = {
                nixpkgs.follows = "nixpkgs";
                flake-utils.follows = "flake-utils";
            };
        };
    };
    outputs = { self, nixpkgs, flake-utils, rust-overlay, ... }:
        flake-utils.lib.eachDefaultSystem
            (system:
                let
                    overlays = [ (import rust-overlay) ];
                    pkgs = import nixpkgs {
                        inherit system overlays;
                    };
                    rustToolchain = pkgs.pkgsBuildHost.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
                    nativeBuildInputs = with pkgs; [ rustToolchain ];
                    buildInputs = with pkgs; [
                        bashInteractive
                        probe-rs
                        cargo-binutils
                        libusb1
                        # elf2uf2-rs
                        # flip-link
                        # picotool
                     ];
                in
                with pkgs;
                {
                    devShells.default = mkShell {
                        inherit buildInputs nativeBuildInputs;
                        shellHook = ''
                        export SHELL=/run/current-system/sw/bin/bash
                        '';
                    };
                }
            );
}
