# pmservers-cli

`pms` is the command line tool for PMServers. It does what the console does: projects, containers, databases and AI keys.

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

`pms login` uses the PMServers device login service. `pms login --email` uses the Supabase project URL and anon key. These are public values and are not stored in this repo.

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
pms catalog [sizes|models]

pms project list | create <name> [--use] | show | rename <name> | delete | use <project> | usage | leave
pms member list | invite <email> | remove <email|user-id>
pms token list | create --name <name> [--expires Nd|never] | revoke <token-id>

pms events [--follow]

pms access show | request <reason>
pms container list | show | create [--port P ...] | edit [--port P ... | --clear-ports] | delete | start | stop | restart | logs [--previous] [--follow]
pms container edit <name> --set-env B=20 --set-env D=4 --unset-env C   # change only these variables
pms container edit <name> --env A=1                                    # replaces ALL variables
pms container create web --image ghcr.io/owner/web:1 --size pms-c-250m-512 --port 3000
pms registry-credential list | add <name> --host H --username U --token-stdin | delete <id>
pms db list | show | create [--vector] | delete | vector on|off | reset-password
pms network show
pms network tailscale enable [--auth-key-file F] | disable
pms network cloudflared enable [--token-file F] | disable
pms network expose <name> [--port N] [--via tailscale|cloudflared|both] [--external-port N]
pms network unexpose <name> [--port N] [--via tailscale|cloudflared|both]
pms ai-key list | show | create --model M --budget USD | edit | rotate | delete
pms storage status | enable | disable
pms storage bucket ls | create <name> | rm <name>
pms storage key ls | create <name> [--read-only] [--bucket B ...] [--env] | reset <key-id> [--env] | rm <key-id>
```

Use `pms <command> --help` for all options.

Global options: `--api-url` (or `PMS_API_URL`, default `https://api.pmservers.org`), `--project` (or `PMS_PROJECT`; an ID or a name; `pms project use` saves a default in `~/.config/pms/config.toml`), `--output table|json` (or `PMS_OUTPUT`), `--yes`.

- A delete asks first. Without a terminal it fails unless you pass `--yes`.
- To share a file, make a presigned URL with any S3 tool, for example `aws s3 presign s3://<bucket>/<key> --expires-in 3600 --endpoint-url https://s3.pmservers.org`. A presigned URL works until it expires. The longest lifetime is 7 days.
- `--wait` on create and on actions waits for the resource, using the event stream. `--timeout` sets the limit in seconds (default 600).
- Secrets (the AI key secret, the database password and connection URL) print one time, as in the console. The API does not return them again.
- A connector token (Tailscale auth key or cloudflared token) comes from standard input or a file with no group or world permissions. It is never accepted as a command-line value. `pms network enable` with a token makes the connector, or replaces its token.

- Object storage needs a sign-in, not an API token: the API answers 403 to tokens. A project member passes `--project <project>` (the owner of the project is the account) or `--account <owner-id>`. Use one of them, not both. A key secret prints one time. `--env` prints `export` lines for the AWS CLI and SDKs, for example `eval "$(pms storage key create ci --env)"`. Storage uses path style only. The first line of the `--env` output says so.
- A project has at most one Tailscale connector and one cloudflared connector. A connector reaches only the targets you choose with `pms network expose`. Use `pms network show` for the status and the address of each target.
- `pms network expose <name>` takes a container or database name (or ID). `--port` is needed when the container has more than one port. `--via both` (the default) uses each connector that is on. `--external-port` sets the port on the tailnet name (Tailscale only); the default is the target port.
- Tailscale is beta. The addresses are on your tailnet, as `pms network show` prints them.
- cloudflared targets are reached through a Cloudflare Tunnel. You set the routes in your Cloudflare dashboard, to the origin that `pms network show` prints.

The session is kept in `~/.config/pms/credentials.json` with mode `0600`. The folder is `~/.config/pms` with mode `0700` when `pms` creates it. `pms` does not use the OS keychain.

After you update from an older version, run `pms login` once. Older versions kept the session in the OS keychain. `pms` does not read that item. You can delete the `pms-cli` item in Keychain Access.

`PMS_NO_KEYRING` has no effect. It is still accepted, so scripts keep working. `PMS_CONFIG_DIR` changes the config folder and `PMS_DATA_DIR` the data folder.

### Device login

`pms login` prints a short code and opens the verification page. You can also run it over SSH or in a container with `--no-browser`, then open the printed URL on another device. Codes expire after 10 minutes and can be used once. Set `PMS_AUTH_URL` to use a local auth service. `--port` was removed because login no longer listens for a browser callback.

## API tokens and CI

An API token lets a script or CI job use one project. It is not a sign-in.

1. Make a token for the project. It is shown one time, so save it now as a secret in your CI settings. Use `--expires` to set a lifetime of 1 to 365 days. The default is `never`.

   ```sh
   pms token create --project slop --name ci --expires 90d
   ```

   Use `-o json` to read the token in a script: `pms -o json token create --project slop --name ci | jq -r .token`.

2. List or revoke tokens with `pms token list --project slop` and `pms token revoke --project slop <token-id>`. Revoke a token that leaks. A project can have 20 active tokens.

3. Set `PMS_TOKEN` to the token. When it is set, `pms` uses it instead of the saved session. `pms` never tries to refresh a token. Set the project too, with `PMS_PROJECT` or `--project`.

   ```yaml
   env:
     PMS_TOKEN: ${{ secrets.PMS_TOKEN }}
     PMS_PROJECT: slop
   steps:
     - run: pms whoami
   ```

A token works for the management API of its project only. The API refuses these for a token: token management, members, AI keys, project create, change and delete, account access, registry credential changes, and admin. Then `pms` shows the API message. Bad, revoked and expired tokens get `The API token is not valid.`. Too many bad checks from one network give a `429` for 10 minutes.

For sandboxes, use the same token as the E2B API key:

```sh
E2B_API_KEY=<token>
E2B_API_URL=https://api.pmservers.org/sandboxes
E2B_SANDBOX_URL=https://sandbox.pmservers.org
```

## Develop

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Tests use mock servers. They do not call Supabase or the real API.
