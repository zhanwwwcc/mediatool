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

    // 生产环境:安装包资源目录
    let resolved = app.path().resolve_resource(&rel);
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
