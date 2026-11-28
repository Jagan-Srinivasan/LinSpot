# Building LinSpot packages

LinSpot is packaged with [nfpm](https://nfpm.goreleaser.com) into `.deb` (Debian/Ubuntu/Mint),
`.rpm` (Fedora/RHEL) and `.pkg.tar.zst` (Arch/CachyOS/Manjaro) packages, plus a portable
x86_64 binary tarball.

## Dependency names per distro (verified in CI)

Each package declares the dependency names of its target distro. Names differ — most
notably the polkit stack:

| What | Debian/Ubuntu (.deb) | Fedora/RHEL (.rpm) | Arch (.pkg.tar.zst) |
|---|---|---|---|
| polkit daemon | `polkitd` | `polkit` | `polkit` |
| pkexec (admin prompt) | `pkexec` (own package since Debian 12 / Ubuntu 22.04) | in `polkit` | in `polkit` |
| dnsmasq | `dnsmasq-base` | `dnsmasq` | `dnsmasq` |
| iproute | `iproute2` | `iproute` | `iproute2` |
| NetworkManager | `network-manager` | `NetworkManager` (+ `NetworkManager-wifi`) | `networkmanager` |
| rfkill | `rfkill` | `rfkill` | in `util-linux` |
| GL/EGL | `libgl1` `libegl1` | `mesa-libGL` `mesa-libEGL` | `mesa` |
| wayland/xkb | `libwayland-client0` `libxkbcommon0` | `libwayland-client` `libxkbcommon` | `wayland` `libxkbcommon` |
| X11 libs | `libx11-6` `libxcursor1` `libxrandr2` `libxi6` | `libX11` `libXcursor` `libXrandr` `libXi` | `libx11` `libxcursor` `libxrandr` `libxi` |

> There is **no `polkit` package on Debian/Ubuntu** — that is the Arch/Fedora name.
> Depending on it there makes the package uninstallable.

The CI workflow installs every package inside its own distro container
(`debian:stable`, `fedora:latest`, `archlinux:base-devel`) before a release is
published, so a wrong name fails the release instead of the user's install.
The .rpm is built for Fedora/RHEL; openSUSE uses different library names
(e.g. `Mesa-libGL1`) — use `install.sh` there.

## Local build

Install Rust/Cargo and nfpm, then run:

```sh
./packaging/build-packages.sh deb    # Debian/Ubuntu
./packaging/build-packages.sh rpm    # Fedora/RHEL
./packaging/build-packages.sh arch   # Arch Linux
./packaging/build-packages.sh all
```

Outputs are written to `dist/`. The version comes from `Cargo.toml`.
Package metadata: `packaging/nfpm-deb.yaml`, `packaging/nfpm-rpm.yaml`,
`packaging/nfpm-arch.yaml`; shared file lists are generated into `pkg/` by the script.

## GitHub Actions

`.github/workflows/release.yml` runs on `workflow_dispatch` and on every `v*` tag push:

1. builds the release binary,
2. packages it as .deb / .rpm / Arch package + portable tarball,
3. smoke-tests each package in its native distro container,
4. publishes the GitHub release with the packages and release notes
   (notes template: `packaging/release-notes.md`).

To release: bump the version in `Cargo.toml`, commit, then
`git tag v0.0.2 && git push origin v0.0.2`.
