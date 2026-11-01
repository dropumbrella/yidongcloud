use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use tao::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy},
    platform::run_return::EventLoopExtRunReturn,
    platform::windows::EventLoopBuilderExtWindows,
    window::WindowBuilder,
};
use webview2_com::GetCookiesCompletedHandler;
use webview2_com::Microsoft::Web::WebView2::Win32::{ICoreWebView2CookieList, ICoreWebView2_2};
use windows::core::{Interface, PCWSTR, PWSTR};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use wry::{WebViewBuilder, WebViewExtWindows};

use crate::client::ClientError;

const LOGIN_URL: &str = "https://yun.139.com/";
const POLL_INTERVAL: Duration = Duration::from_secs(1);
const WINDOW_TIMEOUT: Duration = Duration::from_secs(300);
const PARENT_TIMEOUT: Duration = Duration::from_secs(360);
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const TITLE_WAITING: &str = "139 云盘登录 - 登录完成后请手动关闭窗口";
const TITLE_SUCCESS: &str = "登录成功 - 请关闭窗口以完成";

enum UserEvent {
    Cookies(Option<String>),
}

/// 父进程：spawn 自身作为子进程启动窗口，轮询 token 文件。
///
/// 拿到 token 立刻返回，不 kill 子进程、不等窗口关闭。父进程随后继续
/// login 后续流程并退出；窗口由子进程持有，用户随时关闭都不会影响
/// 已经结束的终端进程。
pub fn interactive_login() -> Result<String, ClientError> {
    let token_file = std::env::temp_dir().join(format!(
        "cloud139_login_{}.token",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&token_file);

    let exe = std::env::current_exe()
        .map_err(|e| ClientError::Other(format!("获取自身路径失败: {}", e)))?;

    let mut child = {
        use std::os::windows::process::CommandExt;
        Command::new(&exe)
            .arg("login")
            .arg("--browser-window")
            .arg(&token_file)
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| ClientError::Other(format!("启动登录窗口失败: {}", e)))?
    };

    let start = Instant::now();
    loop {
        if start.elapsed() > PARENT_TIMEOUT {
            let _ = child.kill();
            let _ = std::fs::remove_file(&token_file);
            return Err(ClientError::Other("浏览器登录超时".to_string()));
        }
        if token_file.exists() {
            if let Ok(content) = std::fs::read_to_string(&token_file) {
                let token = content.trim().to_string();
                if !token.is_empty() {
                    let _ = std::fs::remove_file(&token_file);
                    // 不 kill 子进程：窗口继续存活，用户自行关闭。
                    return Ok(token);
                }
            }
        }
        // 子进程已退出且没留下 token → 用户取消或启动失败
        if let Ok(Some(_)) = child.try_wait() {
            let _ = std::fs::remove_file(&token_file);
            return Err(ClientError::Other(
                "浏览器登录未完成或已取消".to_string(),
            ));
        }
        thread::sleep(Duration::from_millis(500));
    }
}

/// 子进程：创建窗口，轮询 cookie。检测到 authorization 后写入 token 文件，
/// 然后继续保持窗口存活，等用户手动关闭。
pub fn run_window_child(token_file: &Path) -> Result<(), ClientError> {
    // WebView2 要求 STA 线程。
    let com_ok = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok() };
    let result = run_window_inner(token_file);
    if com_ok {
        unsafe { CoUninitialize() };
    }
    result
}

fn run_window_inner(token_file: &Path) -> Result<(), ClientError> {
    let token_file_owned: PathBuf = token_file.to_path_buf();

    let mut builder = EventLoopBuilder::<UserEvent>::with_user_event();
    builder.with_any_thread(true);
    let mut event_loop = builder.build();
    let proxy = event_loop.create_proxy();

    let window = WindowBuilder::new()
        .with_title(TITLE_WAITING)
        .with_inner_size(LogicalSize::new(1100.0, 800.0))
        .with_min_inner_size(LogicalSize::new(800.0, 600.0))
        .build(&event_loop)
        .map_err(|e| ClientError::Other(format!("创建窗口失败: {}", e)))?;

    let webview = WebViewBuilder::new()
        .with_url(LOGIN_URL)
        // 持久化 profile：复用登录缓存，下次打开通常已是登录态。
        .with_user_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
        )
        .build(&window)
        .map_err(|e| {
            ClientError::Other(format!(
                "创建 WebView 失败: {}。请确认已安装 WebView2 Runtime（Win10/11 默认自带）。",
                e
            ))
        })?;

    let start = Instant::now();
    let mut next_poll = Instant::now() + POLL_INTERVAL;
    let mut pending = false;
    let mut found = false;

    event_loop.run_return(move |event, _, control_flow| {
        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                let code = if found { 0 } else { 1 };
                *control_flow = ControlFlow::ExitWithCode(code);
                return;
            }
            Event::MainEventsCleared => {
                let now = Instant::now();
                if !found && now >= next_poll {
                    if start.elapsed() > WINDOW_TIMEOUT {
                        *control_flow = ControlFlow::ExitWithCode(2);
                        return;
                    }
                    if !pending {
                        pending = true;
                        if let Err(_err) = request_cookies(&webview, &proxy) {
                            pending = false;
                        }
                    }
                    next_poll = now + POLL_INTERVAL;
                }
            }
            Event::UserEvent(UserEvent::Cookies(token)) => {
                pending = false;
                if let Some(t) = token {
                    if !found {
                        found = true;
                        let _ = window.set_title(TITLE_SUCCESS);
                        // 写 token 文件，父进程会立即读到并继续。
                        let _ = std::fs::write(&token_file_owned, &t);
                    }
                }
            }
            _ => {}
        }
        // 已拿到 token 后不再轮询，静默等待用户关闭窗口。
        *control_flow = if found {
            ControlFlow::Wait
        } else {
            ControlFlow::WaitUntil(next_poll)
        };
    });

    Ok(())
}

fn request_cookies(
    webview: &wry::WebView,
    proxy: &EventLoopProxy<UserEvent>,
) -> Result<(), ClientError> {
    let webview2 = webview.webview();
    let webview2_2: ICoreWebView2_2 = webview2
        .cast()
        .map_err(|e| ClientError::Other(format!("cast ICoreWebView2_2 失败: {}", e)))?;
    let cookie_manager = unsafe { webview2_2.CookieManager() }
        .map_err(|e| ClientError::Other(format!("获取 CookieManager 失败: {}", e)))?;

    let proxy = proxy.clone();
    let url = windows::core::HSTRING::from(LOGIN_URL);
    let url_pcwstr = PCWSTR::from_raw(url.as_ptr());
    unsafe {
        cookie_manager
            .GetCookies(
                url_pcwstr,
                &GetCookiesCompletedHandler::create(Box::new(move |err, list| {
                    let token = extract_authorization(err, list);
                    let _ = proxy.send_event(UserEvent::Cookies(token));
                    Ok(())
                })),
            )
            .map_err(|e| ClientError::Other(format!("GetCookies 失败: {}", e)))?;
    }
    Ok(())
}

fn extract_authorization(
    err: windows::core::Result<()>,
    list: Option<ICoreWebView2CookieList>,
) -> Option<String> {
    err.ok()?;
    let list = list?;
    let mut count = 0u32;
    unsafe { list.Count(&mut count).ok()? };
    for i in 0..count {
        let Ok(cookie) = (unsafe { list.GetValueAtIndex(i) }) else {
            continue;
        };
        let mut name = PWSTR::null();
        let mut value = PWSTR::null();
        unsafe {
            if cookie.Name(&mut name).is_err() || cookie.Value(&mut value).is_err() {
                continue;
            }
            let name_str = name.to_string().unwrap_or_default();
            let value_str = value.to_string().unwrap_or_default();
            if name_str == "authorization" && !value_str.is_empty() {
                return Some(value_str);
            }
        }
    }
    None
}
