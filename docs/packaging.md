# Packaging and community installs

Official installs: [GitHub Releases](https://github.com/vee-sh/k8pk/releases) (tarballs + `install.sh`), [Homebrew](https://github.com/vee-sh/homebrew-tap) (`brew install vee-sh/tap/k8pk`), and building from source (`cargo install --path rust/k8pk` or `cargo build --release`).

The sections below are **community-maintained patterns**; adapt versions and hashes for your environment.

## Nix

Use the [flake.nix](../flake.nix) at the repository root:

```bash
nix build .#k8pk
./result/bin/k8pk --version
```

Or in a `flake.nix` input:

```nix
inputs.k8pk.url = "github:vee-sh/k8pk";
```

Override the `version` / `rev` in the flake if you track a fork or a specific tag.

## Arch Linux (AUR)

There is no official AUR package in this repo. Use [packaging/aur/PKGBUILD.example](../packaging/aur/PKGBUILD.example) as a starting point: set `pkgver` to the release tag, update `sha256sums` from the release tarball, and publish or install locally with `makepkg`.

## Debian / Ubuntu (.deb)

Build a binary with `cargo build --release`, then place `k8pk` under `/usr/local/bin` or use `cargo deb` (third-party) to package. For a minimal manual install:

```bash
sudo install -m 0755 rust/k8pk/target/release/k8pk /usr/local/bin/k8pk
sudo mkdir -p /usr/local/share/man/man1
sudo install -m 0644 man-out/k8pk.1 /usr/local/share/man/man1/   # if you generated man pages
```

## OpenShift CLI override (OCP)

For CI or non-standard `oc` locations, see README (`K8PK_OC`, `k8pk --oc`, `k8pk info oc`) and `tests/fixtures/fake-oc.sh`.
