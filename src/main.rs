use clap::Parser;

use cloud139::cli::app::Cli;
use cloud139::error;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 无参数启动（双击 exe 或直接执行 cloud139）时进入交互模式。
    if std::env::args_os().len() == 1 {
        if let Err(err) = cloud139::cli::repl::run().await {
            error!("{}", err);
            std::process::exit(cloud139::cli::runner::exit_code(&err));
        }
        return Ok(());
    }

    let cli = Cli::parse();
    if let Some(path) = cli.config.clone() {
        cloud139::config::set_config_override(path);
    }
    cloud139::utils::logger::init_verbose(&cli.verbose);

    if let Err(err) = cloud139::cli::runner::run(cli).await {
        error!("{}", err);
        std::process::exit(cloud139::cli::runner::exit_code(&err));
    }

    Ok(())
}
