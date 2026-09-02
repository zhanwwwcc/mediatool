// 内置 ffmpeg / ffprobe 的定位:
// 1. 生产环境:从安装包的资源目录解析(Windows 为 ffmpeg.exe / ffprobe.exe)
// 2. 开发环境(tauri dev 未打包):回退到编译期记录的 src-tauri/resources 源目录
// 全程使用绝对路径调用,不依赖系统 PATH。

use std::path::PathBuf;
use tauri::{AppHandle, Manager};

/// 解析内置二进制的绝对路径;找不到时返回带提示的错误
pub fn resource_binary(app: &AppHandle, name: &str) -> Result<PathBuf, String> {
    // Windows 上的可执行文件带 .exe 后缀
    let file = if cfg!(target_os = "windows") {
        format!("{}.exe", name)
    } else {
        name.to_string()
    };
    let rel = format!("resources/{}", file);

    // 生产环境:安装包资源目录(Windows 为 exe 旁,macOS 为 .app 内 Resources)
    let resolved = app
        .path()
        .resource_dir()
        .map(|d| d.join(&rel))
        .unwrap_or_else(|_| PathBuf::from(&rel));
    if resolved.exists() {
        return Ok(resolved);
    }

    // 开发环境兜底:CARGO_MANIFEST_DIR 在编译期指向 src-tauri 目录
    if let Some(dir) = option_env!("CARGO_MANIFEST_DIR") {
        let dev_path = PathBuf::from(dir).join(&rel);
        if dev_path.exists() {
            return Ok(dev_path);
        }
    }

    Err(format!(
        "未找到内置 {}。本地开发请先将其放入 src-tauri/resources/ 目录(参见 README)",
        name
    ))
}

/// 统一初始化 ffmpeg/ffprobe 子进程:
/// Windows 下加 CREATE_NO_WINDOW,避免 GUI 程序调用时弹出黑色控制台窗口。
/// 返回配置好的 Command,便于继续链式调用。
#[allow(unused_mut)] // Windows 分支才需要 mut,避免非 Windows 编译产生 unused_mut 警告
pub fn prepare(mut cmd: std::process::Command) -> std::process::Command {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}
