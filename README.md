# pmservers-cli

`pms` is the command line tool for PMServers. It does what the console does: projects, VMs, containers, databases and AI keys.

This repo is private for now. Do not put internal host names or secrets in it.

## Install

Release binaries are on the GitHub releases page of this repo. Only members of the organization can download them for now:

```sh
gh release download --repo pretty-manageable-servers/pmservers-cli --pattern 'pms-*-aarch64-apple-darwin.tar.gz'
```

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

In GitHub Actions, the release workflow reads `PMS_SUPABASE_URL` and `PMS_SUPABASE_ANON_KEY` from the repo variables or secrets of the same names. See [Release](#release).

## Use

```sh
pms login                 # GitHub, in the browser
pms login --email         # email and password, asked in the terminal
pms whoami
pms catalog
```

Global options: `--api-url` (or `PMS_API_URL`, default `https://api.pmservers.org`), `--output table|json` (or `PMS_OUTPUT`), `--yes`.

The session is kept in the OS keychain. If there is no keychain, it is kept in `~/.config/pms/credentials.json` with mode `0600`. Set `PMS_NO_KEYRING=1` to always use the file. `PMS_CONFIG_DIR` changes the config folder.

### Browser login and the redirect URL

`pms login` listens on `http://127.0.0.1:<random port>/callback`. The Supabase project must allow this redirect URL, for example with `http://127.0.0.1:*/**` in the redirect allow list. If it does not, use a fixed port that is on the list: `pms login --port 54321`. Or use `pms login --email`.

## Develop

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Tests use mock servers. They do not call Supabase or the real API.
