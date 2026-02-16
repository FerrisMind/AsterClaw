//! PicoRS - Ultra-lightweight personal AI agent
//! Rust port from Go version

mod agent;
mod auth;
mod bus;
mod channels;
mod config;
mod constants;
mod context_builder;
mod cron;
mod devices;
mod health;
mod heartbeat;
mod logger;
mod memory;
mod migrate;
mod providers;
mod session;
mod skills;
mod state;
mod tools;
mod voice;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::env;
use std::sync::Arc;

#[derive(Parser, Debug)]
#[command(name = "picors")]
#[command(about = "Ultra-lightweight personal AI assistant")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
    #[arg(short, long)]
    debug: bool,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Onboard,
    Agent {
        #[arg(short, long)]
        message: Option<String>,
        #[arg(short, long)]
        session: Option<String>,
    },
    Gateway {
        #[arg(short, long)]
        debug: bool,
    },
    Status,
    Cron {
        #[command(subcommand)]
        command: Option<CronCommands>,
    },
    Migrate {
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        config_only: bool,
        #[arg(long)]
        workspace_only: bool,
        #[arg(long)]
        force: bool,
    },
    Auth {
        #[command(subcommand)]
        command: Option<AuthCommands>,
    },
    Skills {
        #[command(subcommand)]
        command: Option<SkillsCommands>,
    },
    Version,
}

#[derive(Subcommand, Debug)]
enum CronCommands {
    List {
        #[arg(long, default_value_t = false)]
        enabled_only: bool,
    },
    Add {
        #[arg(long)]
        name: String,
        #[arg(long)]
        message: String,
        #[arg(long)]
        every: Option<u64>,
        #[arg(long)]
        cron: Option<String>,
        #[arg(long)]
        channel: Option<String>,
        #[arg(long)]
        chat_id: Option<String>,
        #[arg(long, default_value_t = true)]
        enabled: bool,
    },
    Remove {
        id: String,
    },
    Enable {
        id: String,
    },
    Disable {
        id: String,
    },
}

#[derive(Subcommand, Debug)]
enum AuthCommands {
    Login {
        #[arg(short, long)]
        provider: Option<String>,
        #[arg(long, default_value_t = false)]
        device_code: bool,
        #[arg(long)]
        token: Option<String>,
    },
    Logout {
        #[arg(short, long)]
        provider: Option<String>,
    },
    Status,
}

#[derive(Subcommand, Debug)]
enum SkillsCommands {
    List,
    Install { repo: String },
    Remove { name: String },
    Search,
    Show { name: String },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let log_level = if cli.debug {
        tracing::Level::DEBUG
    } else {
        tracing::Level::INFO
    };
    logger::init(log_level)?;

    match cli.command {
        Commands::Onboard => onboard(),
        Commands::Agent { message, session } => agent_cmd(message, session),
        Commands::Gateway { debug } => gateway_cmd(debug),
        Commands::Status => status_cmd(),
        Commands::Version => {
            println!("🦞 picors {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Commands::Cron { command } => cron_cmd(command),
        Commands::Migrate {
            dry_run,
            config_only,
            workspace_only,
            force,
        } => migrate_cmd(dry_run, config_only, workspace_only, force),
        Commands::Auth { command } => auth_cmd(command),
        Commands::Skills { command } => skills_cmd(command),
    }
}

fn onboard() -> Result<()> {
    let config_path = config::get_config_path()?;

    if config_path.exists() {
        print!(
            "Config already exists at {}. Overwrite? (y/n): ",
            config_path.display()
        );
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if input.trim() != "y" {
            println!("Aborted.");
            return Ok(());
        }
    }

    let cfg = config::Config::default();
    config::save_config(&config_path, &cfg)?;

    // Create workspace templates
    let workspace = cfg.workspace_path();
    create_workspace_templates(&workspace)?;

    println!("🦞 picors is ready!");
    println!("\nNext steps:");
    println!("  1. Add your API key to {}", config_path.display());
    println!("     Get one at: https://openrouter.ai/keys");
    println!("  2. Chat: picors agent -m \"Hello!\"");

    Ok(())
}

fn create_workspace_templates(workspace: &std::path::Path) -> Result<()> {
    use std::fs;

    // Create necessary directories
    let dirs = ["memory", "cron", "agents", "data"];

    for dir in dirs {
        let path = workspace.join(dir);
        fs::create_dir_all(path)?;
    }

    // Create default memory file
    let memory_file = workspace.join("memory/MEMORY.md");
    if !memory_file.exists() {
        fs::write(memory_file, "# Memory\n\nPersonal notes and memories.\n")?;
    }

    Ok(())
}

fn agent_cmd(message: Option<String>, session: Option<String>) -> Result<()> {
    let config_path = config::get_config_path()?;
    let config = config::load_config(&config_path)?;

    // Create provider
    let provider = providers::create_provider(&config)?;

    // Create message bus (wrap in Arc)
    let msg_bus = Arc::new(bus::MessageBus::new());

    // Create agent loop
    let agent_loop = Arc::new(agent::AgentLoop::new(&config, &msg_bus, provider.clone()));

    println!("🦞 Agent initialized");
    println!(
        "  Tools: {} loaded",
        agent_loop.get_startup_info()["tools"]["count"]
    );

    let session_key = session.unwrap_or_else(|| "cli:default".to_string());

    if let Some(msg) = message {
        // Single message mode
        let runtime = tokio::runtime::Runtime::new()?;
        let response =
            runtime.block_on(async { agent_loop.process_direct(&msg, &session_key).await })?;

        println!("\n🦞 {}", response);
    } else {
        // Interactive mode
        println!("🦞 Interactive mode (Ctrl+C to exit)\n");
        interactive_mode(&agent_loop, &session_key)?;
    }

    Ok(())
}

fn interactive_mode(agent_loop: &Arc<agent::AgentLoop>, session_key: &str) -> Result<()> {
    use std::io::{self, Write};

    let runtime = tokio::runtime::Runtime::new()?;

    loop {
        print!("🦞 You: ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        let input = input.trim();
        if input.is_empty() {
            continue;
        }

        if input == "exit" || input == "quit" {
            println!("Goodbye!");
            break;
        }

        let response =
            runtime.block_on(async { agent_loop.process_direct(input, session_key).await });

        match response {
            Ok(resp) => println!("\n🦞 {}\n", resp),
            Err(e) => println!("Error: {}", e),
        }
    }

    Ok(())
}

fn gateway_cmd(_debug: bool) -> Result<()> {
    let config_path = config::get_config_path()?;
    let config = config::load_config(&config_path)?;

    // Create provider
    let provider = providers::create_provider(&config)?;

    // Create message bus (wrap in Arc)
    let msg_bus = Arc::new(bus::MessageBus::new());

    // Create agent loop
    let agent_loop = Arc::new(agent::AgentLoop::new(&config, &msg_bus, provider.clone()));

    // Print status
    println!("\n📦 Agent Status:");
    let startup_info = agent_loop.get_startup_info();
    println!("  • Tools: {} loaded", startup_info["tools"]["count"]);

    // Create channel manager - get enabled channels BEFORE moving
    let channel_manager = Arc::new(channels::ChannelManager::new(&config, &msg_bus)?);
    let enabled_channels = channel_manager.get_enabled_channels();
    agent_loop.set_channel_manager(channel_manager.clone());

    if !enabled_channels.is_empty() {
        println!("✓ Channels enabled: {}", enabled_channels.join(", "));
    } else {
        println!("⚠ Warning: No channels enabled");
    }

    println!(
        "✓ Gateway started on {}:{}",
        config.gateway.host, config.gateway.port
    );
    println!("Press Ctrl+C to stop");
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async move {
        let cron_store_path = config.workspace_path().join("cron/jobs.json");
        let cron_service = cron::CronService::new(&cron_store_path, Some(&config));
        cron_service.start()?;

        let heartbeat_service = heartbeat::HeartbeatService::new(
            config.workspace_path(),
            config.heartbeat.interval as u64,
            config.heartbeat.enabled,
        );
        heartbeat_service.set_bus(&msg_bus);
        heartbeat_service.start()?;

        let mut devices_service = devices::Service::new(
            devices::Config {
                enabled: config.devices.enabled,
                monitor_usb: config.devices.monitor_usb,
            },
            config.workspace_path(),
        );
        devices_service.set_bus(msg_bus.clone());
        devices_service.start().await?;

        channel_manager.start_all().await?;

        let health_server = health::HealthServer::new(&config.gateway.host, config.gateway.port);
        health_server.start().await?;
        println!(
            "✓ Health endpoints available at http://{}:{}/health and /ready",
            config.gateway.host, config.gateway.port
        );

        let agent_task = tokio::spawn({
            let agent_loop = agent_loop.clone();
            async move {
                if let Err(err) = agent_loop.run().await {
                    tracing::error!("agent loop failed: {}", err);
                }
            }
        });

        tokio::signal::ctrl_c().await?;
        println!("\nShutting down...");
        agent_loop.stop();
        channel_manager.stop_all().await?;
        heartbeat_service.stop().await;
        devices_service.stop();
        cron_service.stop();
        health_server.stop().await?;
        msg_bus.close();
        agent_task.abort();
        println!("✓ Gateway stopped");
        Ok::<(), anyhow::Error>(())
    })?;

    Ok(())
}

fn status_cmd() -> Result<()> {
    let config_path = config::get_config_path()?;
    let legacy_path = config::get_legacy_config_path()?;

    println!("🦞 picors Status");
    println!("Version: {}", env!("CARGO_PKG_VERSION"));
    println!();

    if config_path.exists() || legacy_path.exists() {
        let active = if config_path.exists() {
            config_path.clone()
        } else {
            legacy_path.clone()
        };
        println!("Config: {} ✓", active.display());

        let config = config::load_config(&active)?;
        let workspace = config.workspace_path();

        println!(
            "Workspace: {} {}",
            workspace.display(),
            if workspace.exists() { "✓" } else { "✗" }
        );

        println!("Model: {}", config.agents.defaults.model);

        // Show provider status
        let has_openrouter = config.providers.openrouter.api_key.is_some();
        let has_anthropic = config.providers.anthropic.api_key.is_some();
        let has_openai = config.providers.openai.api_key.is_some();
        let has_zhipu = config.providers.zhipu.api_key.is_some();
        let has_groq = config.providers.groq.api_key.is_some();

        println!(
            "OpenRouter API: {}",
            if has_openrouter { "✓" } else { "not set" }
        );
        println!(
            "Anthropic API: {}",
            if has_anthropic { "✓" } else { "not set" }
        );
        println!("OpenAI API: {}", if has_openai { "✓" } else { "not set" });
        println!("Zhipu API: {}", if has_zhipu { "✓" } else { "not set" });
        println!("Groq API: {}", if has_groq { "✓" } else { "not set" });
    } else {
        println!("Config: {} ✗", config_path.display());
        println!("\nRun 'picors onboard' to initialize.");
    }

    Ok(())
}

fn cron_cmd(command: Option<CronCommands>) -> Result<()> {
    let config_path = config::get_config_path()?;
    let config = config::load_config(&config_path)?;
    let cron_store_path = config.workspace_path().join("cron/jobs.json");
    let mut service = cron::CronService::new(&cron_store_path, Some(&config));

    match command {
        Some(CronCommands::List { enabled_only }) => {
            let jobs = service.list_jobs(enabled_only);
            if jobs.is_empty() {
                println!("No scheduled jobs.");
                return Ok(());
            }

            println!("\nScheduled Jobs:");
            for job in jobs {
                println!("  {} - {}", job.name, job.id);
                println!("    Schedule: {}", schedule_display(&job.schedule));
                println!("    Enabled: {}", job.enabled);
                if let Some(channel) = job.channel.as_deref() {
                    println!("    Channel: {}", channel);
                }
                if let Some(chat_id) = job.chat_id.as_deref() {
                    println!("    Chat ID: {}", chat_id);
                }
            }
        }
        Some(CronCommands::Add {
            name,
            message,
            every,
            cron,
            channel,
            chat_id,
            enabled,
        }) => {
            let schedule = match (every, cron) {
                (Some(sec), None) => cron::Schedule::Every(sec),
                (None, Some(expr)) => cron::Schedule::Cron(expr),
                (Some(_), Some(_)) => {
                    return Err(anyhow::anyhow!(
                        "Provide either --every or --cron, not both"
                    ));
                }
                (None, None) => {
                    return Err(anyhow::anyhow!(
                        "Missing schedule: provide --every <seconds> or --cron <expr>"
                    ));
                }
            };

            let job = service.add_job(
                &name,
                schedule,
                &message,
                enabled,
                channel.as_deref(),
                chat_id.as_deref(),
            )?;
            println!("Added cron job: {} ({})", job.name, job.id);
        }
        Some(CronCommands::Remove { id }) => {
            if service.remove_job(&id) {
                println!("Removed cron job: {}", id);
            } else {
                println!("Cron job not found: {}", id);
            }
        }
        Some(CronCommands::Enable { id }) => {
            if let Some(job) = service.enable_job(&id, true) {
                println!("Enabled cron job: {} ({})", job.name, job.id);
            } else {
                println!("Cron job not found: {}", id);
            }
        }
        Some(CronCommands::Disable { id }) => {
            if let Some(job) = service.enable_job(&id, false) {
                println!("Disabled cron job: {} ({})", job.name, job.id);
            } else {
                println!("Cron job not found: {}", id);
            }
        }
        _ => {
            println!("Cron commands:");
            println!("  picors cron list [--enabled-only]");
            println!(
                "  picors cron add --name <name> --message <text> [--every <sec> | --cron <expr>] [--channel <name>] [--chat-id <id>]"
            );
            println!("  picors cron remove <id>");
            println!("  picors cron enable <id>");
            println!("  picors cron disable <id>");
        }
    }

    Ok(())
}

fn schedule_display(schedule: &cron::Schedule) -> String {
    match schedule {
        cron::Schedule::Every(sec) => format!("every {}s", sec),
        cron::Schedule::Cron(expr) => format!("cron {}", expr),
    }
}

fn migrate_cmd(dry_run: bool, config_only: bool, workspace_only: bool, force: bool) -> Result<()> {
    let result =
        migrate::migrate_from_openclaw(dry_run, config_only, workspace_only, force, None, None)?;

    println!("Migration summary:");
    println!("  Config migrated: {}", result.config_migrated);
    println!("  Files copied: {}", result.files_copied);
    println!("  Files skipped: {}", result.files_skipped);
    println!("  Backups created: {}", result.backups_created);
    if !result.warnings.is_empty() {
        println!("Warnings:");
        for warning in result.warnings {
            println!("  - {}", warning);
        }
    }

    Ok(())
}

fn auth_cmd(command: Option<AuthCommands>) -> Result<()> {
    match command {
        Some(AuthCommands::Login {
            provider,
            device_code,
            token,
        }) => {
            let provider = provider.unwrap_or_else(|| "openai".to_string());
            if let Some(token) = token {
                let cred = auth::AuthCredential::from_token(&provider, token, None);
                auth::set_credential(&provider, cred)?;
                println!("Saved token for provider: {}", provider);
            } else if provider == "openai" {
                auth::login_openai(device_code)?;
            } else {
                auth::login_paste_token(&provider)?;
            }
        }
        Some(AuthCommands::Logout { provider }) => {
            if let Some(provider) = provider {
                auth::delete_credential(&provider)?;
                println!("Logged out from provider: {}", provider);
            } else {
                auth::delete_all_credentials()?;
                println!("Logged out from all providers");
            }
        }
        Some(AuthCommands::Status) => {
            auth::show_status()?;
        }
        None => {
            println!("Auth commands:");
            println!("  picors auth login --provider <name> [--token <token>] [--device-code]");
            println!("  picors auth logout [--provider <name>]");
            println!("  picors auth status");
        }
    }
    Ok(())
}

fn skills_cmd(command: Option<SkillsCommands>) -> Result<()> {
    let cfg_path = config::get_config_path()?;
    let cfg = config::load_config(&cfg_path)?;
    let workspace = cfg.workspace_path();
    let loader = skills::SkillsLoader::new(&workspace);
    let installer = skills::SkillInstaller::new(&workspace);

    match command {
        Some(SkillsCommands::List) => {
            let skills = loader.list_skills();
            if skills.is_empty() {
                println!("No skills installed.");
            } else {
                println!("Installed skills:");
                for s in skills {
                    println!(
                        "  - {} [{}] {}",
                        s.name,
                        s.source,
                        if s.description.is_empty() {
                            "(no description)"
                        } else {
                            &s.description
                        }
                    );
                }
            }
        }
        Some(SkillsCommands::Install { repo }) => {
            let rt = tokio::runtime::Runtime::new()?;
            let installed = rt.block_on(async { installer.install_from_github(&repo).await })?;
            println!("Installed skill: {}", installed);
        }
        Some(SkillsCommands::Remove { name }) => {
            installer.uninstall(&name)?;
            println!("Removed skill: {}", name);
        }
        Some(SkillsCommands::Search) => {
            let rt = tokio::runtime::Runtime::new()?;
            let available = rt.block_on(async { installer.list_available_skills().await })?;
            if available.is_empty() {
                println!("No skills found.");
            } else {
                for s in available {
                    println!("  - {} ({})", s.name, s.repository);
                    if !s.description.is_empty() {
                        println!("    {}", s.description);
                    }
                    if !s.author.is_empty() {
                        println!("    by {}", s.author);
                    }
                    if !s.tags.is_empty() {
                        println!("    tags: {}", s.tags.join(", "));
                    }
                }
            }
        }
        Some(SkillsCommands::Show { name }) => {
            if let Some(content) = loader.load_skill(&name) {
                println!("{}", content);
            } else {
                println!("Skill not found: {}", name);
            }
        }
        None => {
            println!("Skills commands:");
            println!("  picors skills list");
            println!("  picors skills install <owner/repo/path>");
            println!("  picors skills remove <name>");
            println!("  picors skills search");
            println!("  picors skills show <name>");
        }
    }
    Ok(())
}
