use clap::{ArgGroup, Args, Parser, Subcommand, ValueEnum};

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
    /// Show sizes, images and AI models you can use.
    Catalog(CatalogArgs),
    /// Projects.
    #[command(subcommand)]
    Project(ProjectCmd),
    /// Members of the current project.
    #[command(subcommand)]
    Member(MemberCmd),
    /// Virtual machines.
    #[command(subcommand)]
    Vm(VmCmd),
    /// Show recent events of the current project, or follow them live.
    Events(EventsArgs),
    /// Print an SSH config for the jump host and your VMs.
    SshConfig(SshConfigArgs),
    /// Container access for your account.
    #[command(subcommand)]
    Access(AccessCmd),
    /// Containers.
    #[command(subcommand)]
    Container(ContainerCmd),
    /// Postgres databases.
    #[command(subcommand)]
    Db(DbCmd),
    /// Keys for the AI API.
    #[command(subcommand, name = "ai-key")]
    AiKey(AiKeyCmd),
    /// Registry logins for private images.
    #[command(subcommand, name = "registry-credential")]
    RegistryCredential(RegistryCmd),
    /// SSH public keys saved on your account.
    #[command(subcommand, name = "ssh-key")]
    SshKey(SshKeyCmd),
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

#[derive(Subcommand)]
pub enum SshKeyCmd {
    /// List saved keys.
    List,
    /// Save a public key. Reads standard input if no key or file is given.
    Add {
        name: String,
        /// Path to a public key file, for example ~/.ssh/id_ed25519.pub.
        #[arg(long, conflicts_with = "key")]
        file: Option<std::path::PathBuf>,
        /// The public key text.
        #[arg(long)]
        key: Option<String>,
    },
    /// Delete a saved key by ID.
    Delete { id: String },
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
    Images,
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

#[derive(Subcommand)]
pub enum VmCmd {
    /// List VMs.
    List,
    /// Make a VM.
    Create(CreateVmArgs),
    /// Show a VM (ID or name).
    Show { vm: String },
    /// Delete a VM.
    Delete {
        vm: String,
        #[command(flatten)]
        wait: WaitArgs,
    },
    /// Start a stopped VM.
    Start {
        vm: String,
        #[command(flatten)]
        wait: WaitArgs,
    },
    /// Stop a VM.
    Stop {
        vm: String,
        #[command(flatten)]
        wait: WaitArgs,
    },
    /// Restart a VM.
    Restart {
        vm: String,
        #[command(flatten)]
        wait: WaitArgs,
    },
    /// Show VM metrics.
    Metrics { vm: String },
    /// Open the serial console. Press Ctrl-] to leave.
    Console { vm: String },
    /// Connect with SSH through the jump host. Installs cloudflared if it is missing.
    Ssh(VmSshArgs),
}

#[derive(Args)]
#[command(group(
    ArgGroup::new("key").required(true).multiple(false)
        .args(["ssh_key", "public_key_file", "generate_key"])
))]
pub struct CreateVmArgs {
    pub name: String,
    /// Size ID. See `pms catalog sizes`.
    #[arg(long)]
    pub size: String,
    /// Image ID. See `pms catalog images`.
    #[arg(long)]
    pub image: String,
    /// Disk size in GB.
    #[arg(long, default_value_t = 10)]
    pub disk_gb: u32,
    /// Use a key saved on your account (ID or name).
    #[arg(long)]
    pub ssh_key: Option<String>,
    /// Use the public key in this file.
    #[arg(long, value_name = "FILE")]
    pub public_key_file: Option<std::path::PathBuf>,
    /// Make a new ed25519 key here. Only the public key is sent.
    #[arg(long)]
    pub generate_key: bool,
    /// Where to write the generated private key. Default: ~/.ssh/pms_<name>_ed25519.
    #[arg(long, value_name = "FILE", requires = "generate_key")]
    pub key_out: Option<std::path::PathBuf>,
    #[command(flatten)]
    pub wait: WaitArgs,
}

#[derive(Args)]
pub struct VmSshArgs {
    pub vm: String,
    /// Private key file to use.
    #[arg(short = 'i', long)]
    pub identity: Option<std::path::PathBuf>,
    /// Login user. Default: the user of the image.
    #[arg(short, long)]
    pub user: Option<String>,
    /// More arguments for ssh, after `--`. For example a command to run.
    #[arg(last = true)]
    pub extra: Vec<String>,
}

#[derive(Args)]
pub struct EventsArgs {
    /// Keep the stream open and print new events.
    #[arg(short, long)]
    pub follow: bool,
}

#[derive(Args)]
pub struct SshConfigArgs {
    /// Jump host name. Default: the one the API reports for your VMs.
    #[arg(long)]
    pub jump_host: Option<String>,
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
    /// Attach a remotely managed Cloudflare Tunnel to this declared port.
    #[arg(long, value_parser = clap::value_parser!(u16).range(1..))]
    pub tunnel_port: Option<u16>,
    /// Read the connector token from a private file (mode 0600 or stricter).
    #[arg(long, value_name = "FILE", conflicts_with = "tunnel_token_stdin")]
    pub tunnel_token_file: Option<std::path::PathBuf>,
    /// Read the connector token from standard input.
    #[arg(long, conflicts_with = "tunnel_token_file")]
    pub tunnel_token_stdin: bool,
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
    /// Change the attached tunnel's origin port.
    #[arg(long, value_parser = clap::value_parser!(u16).range(1..), conflicts_with = "detach_tunnel")]
    pub tunnel_port: Option<u16>,
    /// Replace the connector token from a private file (mode 0600 or stricter).
    #[arg(long, value_name = "FILE", conflicts_with_all = ["tunnel_token_stdin", "detach_tunnel"])]
    pub tunnel_token_file: Option<std::path::PathBuf>,
    /// Replace the connector token from standard input.
    #[arg(long, conflicts_with_all = ["tunnel_token_file", "detach_tunnel"])]
    pub tunnel_token_stdin: bool,
    /// Remove the connector and encrypted token.
    #[arg(long, conflicts_with_all = ["tunnel_port", "tunnel_token_file", "tunnel_token_stdin"])]
    pub detach_tunnel: bool,
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
    /// Open a local port to a database through the SSH jump host. Press Ctrl-C to stop.
    Connect(DbConnectArgs),
}

#[derive(Args)]
pub struct DbConnectArgs {
    /// Database ID or name.
    pub database: String,
    /// Local port to listen on (on localhost only).
    #[arg(long, default_value_t = 5432)]
    pub port: u16,
    /// Private key file to use. It must be a key that you saved or gave to a VM.
    #[arg(short = 'i', long)]
    pub identity: Option<std::path::PathBuf>,
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
