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
    /// Listen on this port for the browser callback. Use it if Supabase does not allow a random port.
    #[arg(long, value_name = "PORT", conflicts_with = "email")]
    pub port: Option<u16>,
    /// Print the sign-in URL and do not open the browser.
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
