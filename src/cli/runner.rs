use anyhow::Result;

use crate::cli::app::{Cli, Commands};

/// 分派并执行一条已解析的 CLI 命令。
/// 供一次性执行（main）与交互模式（repl）共用。
pub async fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Commands::Login(args) => crate::commands::login::execute(args).await,
        Commands::Ls(args) => crate::commands::list::execute(args).await,
        Commands::Upload(args) => crate::commands::upload::execute(args).await,
        Commands::Download(args) => crate::commands::download::execute(args).await,
        Commands::Rm(args) => crate::commands::delete::execute(args).await,
        Commands::Mkdir(args) => crate::commands::mkdir::execute(args).await,
        Commands::Mv(args) => crate::commands::mv::execute(args).await,
        Commands::Cp(args) => crate::commands::cp::execute(args).await,
        Commands::Rename(args) => crate::commands::rename::execute(args).await,
        Commands::Sync(args) => crate::commands::sync::execute(args).await,
    }
}

/// 根据错误类型计算进程退出码（与旧 main 行为保持一致）。
pub fn exit_code(err: &anyhow::Error) -> i32 {
    if let Some(exit) = err.downcast_ref::<crate::commands::CommandExit>() {
        exit.code()
    } else {
        1
    }
}
