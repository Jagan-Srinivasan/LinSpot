# Building LinSpot packages

This project includes an NFPM helper and GitHub Actions workflow for Debian packages and RPMs.

## Local build

Install Rust/Cargo, NFPM, and the GUI build headers required by eframe on your distro.
Then run:

```sh
./packaging/build-packages.sh deb
./packaging/build-packages.sh rpm
# or ./packaging/build-packages.sh all
```

Outputs are written to `dist/`. Build RPMs in a Fedora environment and DEBs on Ubuntu 22.04
for the intended ABI compatibility. Package dependencies are declared in `packaging/nfpm.yaml`.

## GitHub Actions

Commit and push the project, then use Actions > Build LinSpot packages > Run workflow.
Pushing a `v*` tag also triggers the workflow. Artifacts are downloadable from the run.

## Important caveats

- Test both package types in clean virtual machines before publishing.
- The package installs the system-wide executable and polkit policy. Desktop widgets remain per-user
  and are installed from the app's Widgets UI after the package is installed.
- Dependency names vary across distro releases; adjust NFPM metadata as needed.
- This initial packaging scaffold is not yet a verified release build.
