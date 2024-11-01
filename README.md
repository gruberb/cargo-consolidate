# cargo-consolidate

The `cargo-consolidate` is a CLI to easier manage workspace dependencies. It scans all workspace members and checks, depending on the arguments passed, if a dependency is shared across at least two members. If so, it moves it up to the workspace `Cargo.toml` file.

> Warning: There is no path resolution yet. So you most probably have to double check manually.

This is very much a WIP and a first shot in saving a 30 minutes of trying to manually combine workspace dependencies.


```bash
> cargo-consolidate --help
Usage: cargo-consolidate [OPTIONS]

Options:
      --manifest-path <MANIFEST_PATH>  Path to the workspace root Cargo.toml of the project you want to consolidate
      --group-all                      Group dependencies of all members into workspace.dependencies If set to false, just dependencies which are used by 2 or more members are being grouped into workspace.dependencies
  -v, --verbose...                     Increase output verbosity (can be used multiple times)
  -h, --help                           Print help
```

### Installation

You can install `cargo-consolidate` directly from crates.io:

```bash
cargo install cargo-consolidate
```

### Usage

```bash
cargo-consolidate --manifest-path /path/to/your/workspace/Cargo.toml
```

By default, a dependency has to occour at least twice to move it up to the project `Cargo.toml`. If you want to move _EVERY_ dependency up, use the `--group-all` flag:

```bash
cargo-consolidate --manifest-path /path/to/your/workspace/Cargo.toml --group-all
```
