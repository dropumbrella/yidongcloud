use std::io::{self, Write};
use std::path::PathBuf;

use anyhow::Result;
use clap::{CommandFactory, Parser};
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::cli::app::Cli;
use crate::cli::runner;

const PROMPT: &str = "cloud139> ";

/// 交互模式：无参数启动（如双击 exe）时进入。
/// 每行输入等价于省去 cloud139 前缀的一条命令。
pub async fn run() -> Result<()> {
    crate::print!("cloud139 {} - 交互模式", env!("CARGO_PKG_VERSION"));
    crate::print!("help 查看全部命令，exit / quit 退出。");

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    crate::info!("当前目录: {}", cwd.display());
    let cfg = crate::config::Config::config_path();
    if cfg.exists() {
        crate::info!("配置文件: {}", cfg.display());
    } else {
        crate::warn!("未找到配置文件（{}），请先执行 login", cfg.display());
    }
    crate::print!("");

    let mut lines = BufReader::new(tokio::io::stdin()).lines();

    loop {
        // 提示符需要不换行输出并立即刷新，故直接使用 std 的 print!。
        print!("{}", PROMPT);
        io::stdout().flush().ok();

        let line = match lines.next_line().await {
            Ok(Some(l)) => l,
            Ok(None) => {
                crate::print!("");
                break;
            }
            Err(err) => {
                crate::error!("读取输入失败: {}", err);
                break;
            }
        };

        let tokens = match tokenize(line.trim()) {
            Ok(t) => t,
            Err(msg) => {
                crate::error!("{}", msg);
                continue;
            }
        };
        if tokens.is_empty() {
            continue;
        }

        match tokens[0].as_str() {
            "exit" | "quit" | "q" => break,
            "version" | "ver" => {
                crate::print!("cloud139 {}", env!("CARGO_PKG_VERSION"));
                continue;
            }
            "help" | "?" => {
                print_help(tokens.get(1).map(String::as_str));
                continue;
            }
            _ => {}
        }

        let mut argv: Vec<String> = Vec::with_capacity(tokens.len() + 1);
        argv.push("cloud139".to_string());
        argv.extend(tokens);

        let cli = match Cli::try_parse_from(argv.into_iter()) {
            Ok(c) => c,
            Err(err) => {
                // 含 --help / --version 的情形，clap 会自行渲染。
                let _ = err.print();
                continue;
            }
        };

        // 配置文件路径在会话首次指定后生效，后续 -c 不再改变。
        if let Some(path) = &cli.config {
            if crate::config::config_override().is_none() {
                crate::config::set_config_override(path.clone());
            } else if crate::config::config_override() != Some(path.as_path()) {
                crate::warn!("配置文件路径已在会话开始时确定，本次 -c 将被忽略");
            }
        }
        // 日志级别是进程级一次性设置。
        if cli.verbose != "info" {
            crate::utils::logger::init_verbose(&cli.verbose);
        }

        if let Err(err) = runner::run(cli).await {
            crate::error!("{}", err);
        }
        crate::print!("");
    }

    Ok(())
}

fn print_help(subcommand: Option<&str>) {
    let mut cmd = Cli::command();
    match subcommand.and_then(|name| cmd.find_subcommand_mut(name)) {
        Some(sub) => {
            let _ = sub.print_help();
        }
        None => {
            let _ = cmd.print_help();
        }
    }
    crate::print!("");
}

/// 将一行输入拆分为参数列表。
/// 支持单/双引号包裹含空格的参数；不处理反斜杠转义（Windows 路径可原样输入）。
fn tokenize(line: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut has_token = false;

    for ch in line.chars() {
        match ch {
            '\'' if !in_double => {
                in_single = !in_single;
                has_token = true;
            }
            '"' if !in_single => {
                in_double = !in_double;
                has_token = true;
            }
            c if c.is_whitespace() && !in_single && !in_double => {
                if has_token {
                    out.push(std::mem::take(&mut cur));
                    has_token = false;
                }
            }
            c => {
                cur.push(c);
                has_token = true;
            }
        }
    }

    if in_single || in_double {
        return Err("引号未闭合".to_string());
    }
    if has_token {
        out.push(cur);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::tokenize;

    #[test]
    fn splits_by_whitespace() {
        assert_eq!(
            tokenize("login -t abc").unwrap(),
            vec!["login".to_string(), "-t".to_string(), "abc".to_string()]
        );
    }

    #[test]
    fn keeps_quoted_segments() {
        assert_eq!(
            tokenize(r#"upload "my file.txt" /backup/"#).unwrap(),
            vec![
                "upload".to_string(),
                "my file.txt".to_string(),
                "/backup/".to_string()
            ]
        );
    }

    #[test]
    fn windows_path_passthrough() {
        assert_eq!(
            tokenize(r"upload C:\Users\me\a.txt /").unwrap(),
            vec![
                "upload".to_string(),
                r"C:\Users\me\a.txt".to_string(),
                "/".to_string()
            ]
        );
    }

    #[test]
    fn empty_and_blank_lines() {
        assert!(tokenize("").unwrap().is_empty());
        assert!(tokenize("   ").unwrap().is_empty());
    }

    #[test]
    fn rejects_unclosed_quote() {
        assert!(tokenize(r#"login -t "abc"#).is_err());
    }
}
