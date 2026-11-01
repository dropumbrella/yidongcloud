use crate::client::ClientError;
use std::path::Path;

#[cfg(target_os = "windows")]
mod windows;

/// 父进程入口：以独立子进程方式打开 WebView2 登录窗口。
///
/// 返回时 token 已就绪，但窗口仍在子进程中运行；父进程可立即继续
/// 后续流程并退出，窗口不受影响，用户自行关闭即可。
/// - Windows: tao + wry 实现
/// - 其他平台: 返回错误提示手动传入 -t
pub fn interactive_login() -> Result<String, ClientError> {
    #[cfg(target_os = "windows")]
    {
        windows::interactive_login()
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err(ClientError::Other(
            "浏览器登录目前仅支持 Windows，请用 -t <TOKEN> 手动登录".to_string(),
        ))
    }
}

/// 子进程入口：创建窗口，检测到 authorization 后写入 token 文件，
/// 然后继续存活直到用户手动关闭窗口。
pub fn run_window_child(token_file: &Path) -> Result<(), ClientError> {
    #[cfg(target_os = "windows")]
    {
        windows::run_window_child(token_file)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = token_file;
        Err(ClientError::Other("仅 Windows 支持".to_string()))
    }
}
