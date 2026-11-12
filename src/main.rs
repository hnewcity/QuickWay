use quickway::{auth, diagnostics, server, upstream};

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::{net::SocketAddr, path::PathBuf, sync::Arc};

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[arg(long, env = "QUICKWAY_QUICK_BASE")]
    quick_base: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Print a redacted sign-in and upstream diagnostics report (no token values).
    Doctor {
        #[arg(long)]
        online: bool,
        /// Make one small model request (uses quota; also enables online checks).
        #[arg(long, value_name = "ID")]
        probe_model: Option<String>,
        /// Explicitly allow a macOS Keychain authorization dialog.
        #[arg(long)]
        authorize_keychain: bool,
    },
    /// Run the Anthropic-compatible proxy on localhost.
    Serve {
        #[arg(long, default_value = "127.0.0.1:8741")]
        listen: SocketAddr,
        #[arg(long, env = "QUICKWAY_API_KEY")]
        api_key: Option<String>,
        #[arg(long, default_value = ".quickway/proxy.key")]
        key_file: PathBuf,
    },
}
#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let base = cli.quick_base.map(Ok).unwrap_or_else(auth::quick_base)?;
    match cli.command {
        Command::Doctor {
            online,
            probe_model,
            authorize_keychain,
        } => {
            let upstream = upstream::Upstream::new(base)?;
            if authorize_keychain {
                upstream.sessions.authorize(&upstream.client).await?;
            }
            let mut report = diagnostics::report(
                &upstream,
                env!("CARGO_PKG_VERSION"),
                online || probe_model.is_some(),
            )
            .await;
            if let Some(model) = probe_model {
                report["inference"] = diagnostics::probe_model(&upstream, &model).await;
                report["recent_errors"] = upstream.recent_errors().into();
            }
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Command::Serve {
            listen,
            api_key,
            key_file,
        } => {
            anyhow::ensure!(
                listen.ip().is_loopback(),
                "QuickWay only binds a loopback address"
            );
            let key = if let Some(key) = api_key {
                key
            } else {
                load_or_create_key(&key_file)?
            };
            anyhow::ensure!(
                key.len() >= 16,
                "Local API key must contain at least 16 characters"
            );
            let app = Arc::new(server::App {
                upstream: upstream::Upstream::new(base)?,
                key,
            });
            let listener = tokio::net::TcpListener::bind(listen).await?;
            eprintln!(
                "QuickWay listening on http://{listen}; local key file: {}",
                key_file.display()
            );
            axum::serve(listener, server::router(app.clone()))
                .with_graceful_shutdown(async move {
                    let _ = tokio::signal::ctrl_c().await;
                    app.upstream.cloud.shutdown().await;
                })
                .await?;
        }
    }
    Ok(())
}

fn load_or_create_key(path: &std::path::Path) -> Result<String> {
    use std::io::Write;
    match std::fs::read_to_string(path) {
        Ok(s) => return Ok(s.trim().to_string()),
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
        _ => {}
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let key = format!(
        "qw_{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(key.as_bytes())?;
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doctor_inference_is_explicit_and_requires_a_model_id() {
        assert!(matches!(
            Cli::try_parse_from(["quickway", "doctor", "--online"])
                .unwrap()
                .command,
            Command::Doctor {
                online: true,
                probe_model: None,
                ..
            }
        ));
        assert!(
            matches!(Cli::try_parse_from(["quickway", "doctor", "--probe-model", "claude-sonnet-4-6"]).unwrap().command,
            Command::Doctor { probe_model: Some(model), .. } if model == "claude-sonnet-4-6")
        );
        assert!(Cli::try_parse_from(["quickway", "doctor", "--probe-model"]).is_err());
    }
}
