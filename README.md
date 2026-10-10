# pmservers-cli

`pms` is the command line tool for PMServers. It does what the console does: projects, VMs, containers, databases and AI keys.

This repo is open source under the MIT license. Do not put internal host names or secrets in it.

## Install

Release binaries are on the GitHub releases page of this repo, for macOS (arm64, x86_64) and Linux (x86_64, arm64).

```sh
gh release download --repo pretty-manageable-servers/pmservers-cli --pattern 'pms-*-aarch64-apple-darwin.tar.gz'
tar -xzf pms-*-aarch64-apple-darwin.tar.gz
install pms-*/pms ~/.local/bin/pms
```

The file names are `pms-<tag>-<target>.tar.gz`. The targets are `aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu`. `SHA256SUMS` lists the checksums. The macOS binaries are not signed or notarized.

## Build

You need Rust stable.

```sh
cargo build --release
```

### Supabase settings

`pms login` needs the Supabase project URL and the anon key. Both are public values. They are not in this repo, because we cannot check them from here.

A release build must have them. Set them when you build:

```sh
PMS_SUPABASE_URL=https://<ref>.supabase.co PMS_SUPABASE_ANON_KEY=<anon key> cargo build --release
```

The values are read with `option_env!`. Variables with the same names at run time win over the built-in values. This is also how you point a dev build at another Supabase project.

## Release

1. Set the version in the root `Cargo.toml` (`[workspace.package]`) and merge it.
2. Set the repository variables `PMS_SUPABASE_URL` and `PMS_SUPABASE_ANON_KEY` (Settings, Secrets and variables, Actions). Secrets with the same names also work. The workflow stops if they are empty.
3. Push a tag that matches the version in `Cargo.toml`: `git tag vX.Y.Z && git push origin vX.Y.Z`.

`.github/workflows/release.yml` runs the checks, builds the four targets, and makes the GitHub release with the archives and `SHA256SUMS`. The tag must match the version in `Cargo.toml`. A tag with a `-` (for example `v0.2.0-rc1`) makes a pre-release. The arm64 Linux binary is cross-compiled on an x86_64 runner.

## Use

```sh
pms login                         # GitHub, in the browser
pms login --email                 # email and password, asked in the terminal
pms whoami
pms catalog [sizes|images|models]

pms project list | create <name> [--use] | show | rename <name> | delete | use <project> | usage | leave
pms member list | invite <email> | remove <email|user-id>
pms ssh-key list | add <name> [--file F | --key K] | delete <id>

pms vm list | show <vm> | delete <vm> | start|stop|restart <vm> | metrics <vm>
pms vm create <name> --size S --image I [--disk-gb N] (--ssh-key K | --public-key-file F | --generate-key [--key-out F]) [--wait]
pms vm console <vm>               # Ctrl-] leaves
pms vm ssh <vm> [-i KEY] [-u USER] [-- ssh args]
pms ssh-config                    # the jump host block and one block for each VM
pms events [--follow]

pms access show | request <reason>
pms container list | show | create | edit | delete | start | stop | restart | logs [--previous] [--follow]
pms registry-credential list | add <name> --host H --username U --token-stdin | delete <id>
pms db list | show | create [--vector] | delete | vector on|off | reset-password
pms ai-key list | show | create --model M --budget USD | edit | rotate | delete
```

Use `pms <command> --help` for all options.

Global options: `--api-url` (or `PMS_API_URL`, default `https://api.pmservers.org`), `--project` (or `PMS_PROJECT`; an ID or a name; `pms project use` saves a default in `~/.config/pms/config.toml`), `--output table|json` (or `PMS_OUTPUT`), `--yes`.

- A delete asks first. Without a terminal it fails unless you pass `--yes`.
- `--wait` on create and on actions waits for the resource, using the event stream. `--timeout` sets the limit in seconds (default 600).
- Secrets (the AI key secret, the database password and connection URL) print one time, as in the console. The API does not return them again.
- `pms vm ssh` uses `ssh` with `cloudflared access ssh` as the proxy for the jump host. If `cloudflared` is not on `PATH`, `pms` asks, then downloads a pinned version into `~/.local/share/pms/bin` (the OS data folder on macOS) and checks its SHA-256 against a value pinned in `crates/pms/src/cloudflared.rs`. `--yes` skips the question. To bump the version, change `VERSION` and the four checksums, then run `cargo test -p pms -- --ignored real_downloads`.

The session is kept in the OS keychain. If there is no keychain, it is kept in `~/.config/pms/credentials.json` with mode `0600`. Set `PMS_NO_KEYRING=1` to always use the file. `PMS_CONFIG_DIR` changes the config folder and `PMS_DATA_DIR` the data folder.

### Browser login and the redirect URL

`pms login` listens on `http://127.0.0.1:<random port>/callback`. The Supabase project must allow this redirect URL, for example with `http://127.0.0.1:*/**` in the redirect allow list. If it does not, use a fixed port that is on the list: `pms login --port 54321`. Or use `pms login --email`.

## Develop

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Tests use mock servers. They do not call Supabase or the real API.
