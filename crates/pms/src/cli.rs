use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::config::DEFAULT_API_URL;
use crate::output::Format;

#[derive(Parser)]
#[command(
    name = "pms",
    version,
    about = "Command line tool for PMServers",
    propagate_version = true
)]
pub struct Cli {
    /// API address. Use http://localhost for a local OrbStack stack.
    #[arg(long, global = true, env = "PMS_API_URL", default_value = DEFAULT_API_URL)]
    pub api_url: String,

    /// Output format. Use json in scripts.
    #[arg(short, long, global = true, env = "PMS_OUTPUT", value_enum, default_value_t = Format::Table)]
    pub output: Format,

    /// Project ID or name. Default: the project set with `pms project use`.
    #[arg(short, long, global = true, env = "PMS_PROJECT")]
    pub project: Option<String>,

    /// Do not ask for confirmation.
    #[arg(short, long, global = true)]
    pub yes: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Sign in. Opens the browser for GitHub, or use --email.
    Login(LoginArgs),
    /// Sign out and remove the saved session.
    Logout,
    /// Show the signed-in user.
    Whoami,
    /// Show sizes and AI models you can use.
    Catalog(CatalogArgs),
    /// Projects.
    #[command(subcommand)]
    Project(ProjectCmd),
    /// Members of the current project.
    #[command(subcommand)]
    Member(MemberCmd),
    /// Show recent events of the current project, or follow them live.
    Events(EventsArgs),
    /// Container access for your account.
    #[command(subcommand)]
    Access(AccessCmd),
    /// Containers.
    #[command(subcommand)]
    Container(Box<ContainerCmd>),
    /// Postgres databases.
    #[command(subcommand)]
    Db(DbCmd),
    /// Project network: Tailscale and cloudflared connectors, and what they reach.
    #[command(subcommand)]
    Network(NetworkCmd),
    /// Keys for the AI API.
    #[command(subcommand, name = "ai-key")]
    AiKey(AiKeyCmd),
    /// Registry logins for private images.
    #[command(subcommand, name = "registry-credential")]
    RegistryCredential(RegistryCmd),
    /// API tokens for scripts and CI. Set PMS_TOKEN to sign in with one.
    #[command(subcommand)]
    Token(TokenCmd),
    /// Object storage (S3 compatible). Sign in with `pms login`: API tokens cannot use it.
    #[command(subcommand)]
    Storage(StorageCmd),
}

#[derive(Subcommand)]
pub enum TokenCmd {
    /// List the API tokens of the project.
    List,
    /// Make a token for the project. The token is shown one time.
    Create {
        /// A name for the token, for example ci.
        #[arg(long)]
        name: String,
        /// How long the token lasts: 1d to 365d, or never.
        #[arg(long, value_name = "Nd|never", default_value = "never", value_parser = parse_expiry)]
        expires: Expiry,
    },
    /// Revoke a token by ID. Anything that uses it stops working.
    Revoke { token_id: String },
}

/// The lifetime of an API token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Expiry {
    Never,
    Days(u32),
}

impl Expiry {
    /// `None` is never. The API reads `null` as never.
    pub fn days(self) -> Option<u32> {
        match self {
            Expiry::Never => None,
            Expiry::Days(n) => Some(n),
        }
    }
}

fn parse_expiry(text: &str) -> Result<Expiry, String> {
    let text = text.trim();
    if text.eq_ignore_ascii_case("never") {
        return Ok(Expiry::Never);
    }
    text.strip_suffix('d')
        .and_then(|n| n.parse::<u32>().ok())
        .filter(|n| (1..=365).contains(n))
        .map(Expiry::Days)
        .ok_or_else(|| format!("{text:?} is not valid. Use 1d to 365d, or never."))
}

#[derive(Subcommand)]
pub enum ProjectCmd {
    /// List your projects.
    List,
    /// Make a project.
    Create {
        name: String,
        /// Also make it the default project.
        #[arg(long = "use")]
        use_it: bool,
    },
    /// Show the current project.
    Show,
    /// Rename the current project.
    Rename { name: String },
    /// Delete the current project. It must have no resources.
    Delete,
    /// Save a default project (ID or name).
    Use { project: String },
    /// Show usage and limits of the current project.
    Usage,
    /// Leave the current project (members only).
    Leave,
}

#[derive(Subcommand)]
pub enum MemberCmd {
    /// List the owner and the members.
    List,
    /// Add a member by email. A new email gets an invite.
    Invite { email: String },
    /// Remove a member by email or user ID.
    Remove { member: String },
}

#[derive(Args)]
pub struct LoginArgs {
    /// Sign in with email and password. The CLI asks for both.
    #[arg(long)]
    pub email: bool,
    /// Print the sign-in URL and code without opening the browser.
    #[arg(long, conflicts_with = "email")]
    pub no_browser: bool,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum CatalogKind {
    All,
    Sizes,
    Models,
}

#[derive(Args)]
pub struct CatalogArgs {
    #[arg(value_enum, default_value_t = CatalogKind::All)]
    pub what: CatalogKind,
}

#[derive(Args, Clone)]
pub struct WaitArgs {
    /// Wait until the resource is ready.
    #[arg(long)]
    pub wait: bool,
    /// Give up waiting after this many seconds.
    #[arg(long, default_value_t = 600, requires = "wait")]
    pub timeout: u64,
}

#[derive(Args)]
pub struct EventsArgs {
    /// Keep the stream open and print new events.
    #[arg(short, long)]
    pub follow: bool,
}

#[derive(Subcommand)]
pub enum AccessCmd {
    /// Show the state of your container access.
    Show,
    /// Ask for container access. Say what you want to run.
    Request { reason: String },
}

#[derive(Subcommand)]
pub enum ContainerCmd {
    /// List containers.
    List,
    /// Run a container.
    Create(CreateContainerArgs),
    /// Show a container (ID or name).
    Show { container: String },
    /// Change the image, size, credential, variables, ports or runtime. This redeploys the container.
    Edit(EditContainerArgs),
    /// Delete a container.
    Delete {
        container: String,
        #[command(flatten)]
        wait: WaitArgs,
    },
    /// Start a stopped container.
    Start {
        container: String,
        #[command(flatten)]
        wait: WaitArgs,
    },
    /// Stop a container.
    Stop {
        container: String,
        #[command(flatten)]
        wait: WaitArgs,
    },
    /// Restart a container.
    Restart {
        container: String,
        #[command(flatten)]
        wait: WaitArgs,
    },
    /// Show the logs.
    Logs(LogsArgs),
}

#[derive(Args)]
pub struct EnvArgs {
    /// An environment variable, KEY=VALUE. Repeat for more.
    #[arg(short = 'e', long = "env", value_name = "KEY=VALUE")]
    pub env: Vec<String>,
    /// A file with KEY=VALUE lines.
    #[arg(long, value_name = "FILE")]
    pub env_file: Option<std::path::PathBuf>,
}

#[derive(Args)]
pub struct CreateContainerArgs {
    pub name: String,
    /// Image, for example ghcr.io/owner/app:1.0.
    #[arg(long)]
    pub image: String,
    /// Size ID. See `pms catalog sizes`.
    #[arg(long)]
    pub size: String,
    /// Registry credential for a private image (ID or name).
    #[arg(long)]
    pub registry_credential: Option<String>,
    /// Runtime. native needs a permission on your account.
    #[arg(long, value_parser = ["gvisor", "native"])]
    pub runtime: Option<String>,
    /// A private TCP port, 1 to 65535. Repeat for more (5 at most). Only containers in the same
    /// project can connect, as NAME:PORT.
    #[arg(long = "port", value_name = "PORT", value_parser = clap::value_parser!(u16).range(1..))]
    pub ports: Vec<u16>,
    #[command(flatten)]
    pub env: EnvArgs,
    #[command(flatten)]
    pub wait: WaitArgs,
}

#[derive(Args)]
pub struct EditContainerArgs {
    pub container: String,
    #[arg(long)]
    pub image: Option<String>,
    #[arg(long)]
    pub size: Option<String>,
    #[arg(long, value_parser = ["gvisor", "native"])]
    pub runtime: Option<String>,
    /// The private TCP ports. These ports replace ALL ports. Repeat for more (5 at most).
    #[arg(long = "port", value_name = "PORT", value_parser = clap::value_parser!(u16).range(1..), conflicts_with = "clear_ports")]
    pub ports: Vec<u16>,
    /// Remove all ports.
    #[arg(long)]
    pub clear_ports: bool,
    /// Registry credential (ID or name).
    #[arg(long, conflicts_with = "no_registry_credential")]
    pub registry_credential: Option<String>,
    /// Remove the registry credential.
    #[arg(long)]
    pub no_registry_credential: bool,
    /// With --env or --env-file: these variables replace ALL variables. To change only some,
    /// use --set-env and --unset-env. Without them, the variables stay as they are.
    #[command(flatten)]
    pub env: EnvArgs,
    /// Remove all environment variables.
    #[arg(long, conflicts_with_all = ["env", "env_file"])]
    pub clear_env: bool,
    /// Add or change one variable, KEY=VALUE. Repeat for more. The other variables stay.
    #[arg(
        long,
        value_name = "KEY=VALUE",
        conflicts_with_all = ["env", "env_file", "clear_env"]
    )]
    pub set_env: Vec<String>,
    /// Like --set-env, but read the variables from a file with KEY=VALUE lines.
    #[arg(
        long,
        value_name = "FILE",
        conflicts_with_all = ["env", "env_file", "clear_env"]
    )]
    pub set_env_file: Option<std::path::PathBuf>,
    /// Remove one variable by name. Repeat for more. The other variables stay.
    #[arg(
        long,
        value_name = "KEY",
        conflicts_with_all = ["env", "env_file", "clear_env"]
    )]
    pub unset_env: Vec<String>,
    #[command(flatten)]
    pub wait: WaitArgs,
}

#[derive(Args)]
pub struct LogsArgs {
    pub container: String,
    /// How many lines.
    #[arg(long, default_value_t = 200, value_parser = clap::value_parser!(u32).range(1..=2000))]
    pub tail: u32,
    /// The run before the last restart. Use it after a crash.
    #[arg(long)]
    pub previous: bool,
    /// Keep printing new lines. Checks every 5 seconds.
    #[arg(short, long)]
    pub follow: bool,
}

#[derive(Subcommand)]
pub enum RegistryCmd {
    /// List saved registry logins.
    List,
    /// Save a registry login. The token is read from a prompt or from standard input.
    Add {
        name: String,
        /// Registry host, for example ghcr.io.
        #[arg(long)]
        host: String,
        #[arg(long)]
        username: String,
        /// Read the token from standard input.
        #[arg(long)]
        token_stdin: bool,
    },
    /// Delete a saved registry login (ID or name).
    Delete { credential: String },
}

#[derive(Subcommand)]
pub enum DbCmd {
    /// List databases.
    List,
    /// Make a database. The password is shown one time.
    Create {
        name: String,
        /// Size ID. See `pms catalog sizes`. Default: the first size.
        #[arg(long)]
        size: Option<String>,
        /// Turn on the pgvector extension.
        #[arg(long)]
        vector: bool,
        #[command(flatten)]
        wait: WaitArgs,
    },
    /// Show a database (ID or name).
    Show { database: String },
    /// Delete a database. There are no backups.
    Delete {
        database: String,
        #[command(flatten)]
        wait: WaitArgs,
    },
    /// Turn the pgvector extension on or off.
    #[command(subcommand)]
    Vector(VectorCmd),
    /// Make a new password. The password is shown one time.
    ResetPassword { database: String },
}

#[derive(Subcommand)]
pub enum NetworkCmd {
    /// Show the connectors, their status and their targets with addresses.
    Show,
    /// The Tailscale connector. Its targets are reachable on your tailnet.
    #[command(subcommand)]
    Tailscale(TailscaleCmd),
    /// The cloudflared connector. Its targets are reached through a Cloudflare Tunnel.
    #[command(subcommand)]
    Cloudflared(CloudflaredCmd),
    /// Make a container or database reachable through a connector.
    Expose(ExposeArgs),
    /// Stop a container or database from being reachable through a connector.
    Unexpose(UnexposeArgs),
}

#[derive(Subcommand)]
pub enum TailscaleCmd {
    /// Turn on the Tailscale connector, or replace its auth key. Reads the key from standard input
    /// unless --auth-key-file is given.
    Enable {
        /// Read the auth key from a private file (mode 0600 or stricter).
        #[arg(long, value_name = "FILE")]
        auth_key_file: Option<std::path::PathBuf>,
    },
    /// Turn off the Tailscale connector. Its targets stop being reachable.
    Disable,
}

#[derive(Subcommand)]
pub enum CloudflaredCmd {
    /// Turn on the cloudflared connector, or replace its token. Reads the token from standard input
    /// unless --token-file is given.
    Enable {
        /// Read the connector token from a private file (mode 0600 or stricter).
        #[arg(long, value_name = "FILE")]
        token_file: Option<std::path::PathBuf>,
    },
    /// Turn off the cloudflared connector. Its targets stop being reachable.
    Disable,
}

/// Which connector `expose` and `unexpose` use.
#[derive(Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum Via {
    Tailscale,
    Cloudflared,
    /// Each connector that is on.
    Both,
}

#[derive(Args)]
pub struct ExposeArgs {
    /// Container or database name, or its ID.
    pub name: String,
    /// The port to expose. Needed when the container has more than one port.
    #[arg(long, value_parser = clap::value_parser!(u16).range(1..))]
    pub port: Option<u16>,
    /// The connector to use.
    #[arg(long, value_enum, default_value_t = Via::Both)]
    pub via: Via,
    /// Tailscale only: the port on the tailnet name. Default: the same as --port.
    #[arg(long, value_parser = clap::value_parser!(u16).range(1..))]
    pub external_port: Option<u16>,
}

#[derive(Args)]
pub struct UnexposeArgs {
    /// Container or database name, or its ID.
    pub name: String,
    /// The port to stop exposing. Default: every port of the container or database.
    #[arg(long, value_parser = clap::value_parser!(u16).range(1..))]
    pub port: Option<u16>,
    /// The connector to use.
    #[arg(long, value_enum, default_value_t = Via::Both)]
    pub via: Via,
}

#[derive(Subcommand)]
pub enum VectorCmd {
    /// Turn pgvector on.
    On { database: String },
    /// Turn pgvector off. It fails if a column or index uses it.
    Off { database: String },
}

#[derive(Subcommand)]
pub enum AiKeyCmd {
    /// List keys.
    List,
    /// Make a key. The secret is shown one time.
    Create {
        name: String,
        /// A model ID. Repeat for more. See `pms catalog models`.
        #[arg(long = "model", required = true)]
        models: Vec<String>,
        /// Spending limit in USD.
        #[arg(long)]
        budget: f64,
    },
    /// Show a key (ID or name).
    Show { key: String },
    /// Change the models or the budget.
    Edit {
        key: String,
        /// The new list of models. Repeat for more. It replaces the old list.
        #[arg(long = "model")]
        models: Vec<String>,
        /// The new spending limit in USD.
        #[arg(long)]
        budget: Option<f64>,
    },
    /// Make a new secret. The old secret stops working. The secret is shown one time.
    Rotate { key: String },
    /// Delete a key.
    Delete { key: String },
}

#[derive(Subcommand)]
pub enum StorageCmd {
    /// Show the object storage: state, usage, limits and the endpoint.
    Status(AccountArgs),
    /// Turn on object storage. Only approved accounts can.
    Enable(AccountArgs),
    /// Turn off object storage. Remove all buckets first. The data is gone.
    Disable(AccountArgs),
    /// Buckets. To share a file, make a presigned URL with any S3 tool.
    #[command(subcommand)]
    Bucket(BucketCmd),
    /// Access keys for the S3 API.
    #[command(subcommand)]
    Key(StorageKeyCmd),
}

/// Storage routes need the owner's account. A project member passes the owner's ID, or the project.
#[derive(Args)]
pub struct AccountArgs {
    /// The owner ID of the account. For a project member. Default: the owner of the current project (`pms project use`), else your own account.
    /// Cannot be used with --project.
    #[arg(long, value_name = "OWNER_ID")]
    pub account: Option<String>,
}

/// True when `--project` was typed on the command line, at any level. Clap counts a value from
/// `PMS_PROJECT` as present too, so a conflict check in clap would also fire on the environment.
pub fn project_typed(matches: &clap::ArgMatches) -> bool {
    matches.value_source("project") == Some(clap::parser::ValueSource::CommandLine)
        || matches
            .subcommand()
            .is_some_and(|(_, sub)| project_typed(sub))
}

#[derive(Subcommand)]
pub enum BucketCmd {
    /// List buckets.
    #[command(name = "ls")]
    List(AccountArgs),
    /// Make a bucket. Bucket names are unique across all of PMServers.
    Create {
        name: String,
        #[command(flatten)]
        acct: AccountArgs,
    },
    /// Delete an empty bucket.
    #[command(name = "rm")]
    Delete {
        name: String,
        #[command(flatten)]
        acct: AccountArgs,
    },
}

#[derive(Subcommand)]
pub enum StorageKeyCmd {
    /// List access keys. The secrets are not shown.
    #[command(name = "ls")]
    List(AccountArgs),
    /// Make an access key. The secret is shown one time.
    Create(CreateStorageKeyArgs),
    /// Make a new key ID and secret with the same settings. The old key stops working.
    Reset {
        key_id: String,
        /// Print export lines for the AWS CLI and SDKs, not a table.
        #[arg(long)]
        env: bool,
        #[command(flatten)]
        acct: AccountArgs,
    },
    /// Delete an access key. Apps that use it stop working.
    #[command(name = "rm")]
    Delete {
        key_id: String,
        #[command(flatten)]
        acct: AccountArgs,
    },
}

#[derive(Args)]
pub struct CreateStorageKeyArgs {
    /// A name for the key, for example ci.
    pub name: String,
    /// The key can read, but not write or delete.
    #[arg(long)]
    pub read_only: bool,
    /// A bucket the key can use. Repeat for more. Default: all buckets, now and later.
    #[arg(long = "bucket", value_name = "BUCKET")]
    pub buckets: Vec<String>,
    /// Print export lines for the AWS CLI and SDKs, not a table.
    #[arg(long)]
    pub env: bool,
    #[command(flatten)]
    pub acct: AccountArgs,
}
