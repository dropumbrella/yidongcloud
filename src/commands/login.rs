use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug, Clone)]
pub struct LoginArgs {
    #[arg(short, long, help = "Authorization Token（不传则打开浏览器登录窗口）")]
    pub token: Option<String>,

    #[arg(
        short,
        long,
        default_value = "personal_new",
        help = "存储类型: personal_new, family, group"
    )]
    pub storage_type: String,

    #[arg(long, help = "云盘ID (家庭云/群组云时需要)")]
    pub cloud_id: Option<String>,

    /// 内部使用：以子进程形式运行登录窗口，token 写入指定文件。
    #[arg(long, hide = true)]
    pub browser_window: Option<PathBuf>,
}

pub async fn execute(args: LoginArgs) -> anyhow::Result<()> {
    // 子进程模式：开窗口写 token，等用户关窗后退出。
    if let Some(path) = args.browser_window {
        tokio::task::spawn_blocking(move || {
            crate::client::browser_login::run_window_child(&path)
        })
        .await
        .map_err(|e| anyhow::anyhow!("窗口线程异常: {}", e))??;
        return Ok(());
    }

    // 手动传入 token。
    if let Some(t) = args.token {
        crate::services::login_service::login(&t, &args.storage_type, args.cloud_id.as_deref())
            .await?;
        return Ok(());
    }

    // 浏览器登录：拿到 token 立刻继续，不等窗口关闭。
    crate::info!("正在打开浏览器窗口，请在窗口中完成登录 ...");
    let token = tokio::task::spawn_blocking(crate::client::browser_login::interactive_login)
        .await
        .map_err(|e| anyhow::anyhow!("窗口线程异常: {}", e))??;
    crate::services::login_service::login(&token, &args.storage_type, args.cloud_id.as_deref())
        .await?;
    Ok(())
}
