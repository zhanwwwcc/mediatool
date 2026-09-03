// 应用入口:注册插件与后端命令

mod crop;
mod ffbin;
mod media;
mod thumb;

use std::path::{Path, PathBuf};
use std::process::Command;

use tauri_plugin_dialog::DialogExt;

/// 打开系统文件选择器(支持多选),返回所选文件的绝对路径列表。
/// 使用非阻塞回调 API(官方推荐用法),不占线程、不阻塞主循环,
/// 彻底规避 blocking_* 系列在部分环境下的卡死问题。
#[tauri::command]
async fn open_files(app: tauri::AppHandle) -> Result<Vec<String>, String> {
    let (tx, mut rx) = tauri::async_runtime::channel(1);
    app.dialog()
        .file()
        .set_title("选择媒体文件")
        .add_filter(
            "媒体文件",
            &[
                "mp4", "mkv", "mov", "avi", "webm", "m4v", "flv", "ts", "mpg", "mpeg",
                "m2ts", "wmv", "3gp", "mp3", "flac", "wav", "aac", "m4a", "ogg", "oga",
                "opus", "aiff", "aif", "wv", "ape", "amr", "caf", "wma",
            ],
        )
        .add_filter("所有文件", &["*"])
        .pick_files(move |files| {
            // 回调在主线程执行;发送失败(如应用退出)直接忽略
            let _ = tx.blocking_send(files);
        });

    // 等待用户选择;取消时回调返回 None。通道关闭(发送端被丢弃)说明对话框异常
    let picked = rx
        .recv()
        .await
        .ok_or_else(|| "文件对话框异常:内部通道已关闭".to_string())?;

    match picked {
        Some(files) => Ok(files
            .into_iter()
            .filter_map(|f| f.as_path().map(|p| p.to_string_lossy().into_owned()))
            .collect()),
        // 用户取消选择不算错误
        None => Ok(Vec::new()),
    }
}

/// 打开系统文件夹选择器,返回所选文件夹的绝对路径;取消返回 None。
/// 同样使用非阻塞回调模式,不阻塞主线程。
#[tauri::command]
async fn open_folder(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let (tx, mut rx) = tauri::async_runtime::channel(1);
    app.dialog()
        .file()
        .set_title("选择输出文件夹")
        .pick_folder(move |folder| {
            let _ = tx.blocking_send(folder);
        });

    let picked = rx
        .recv()
        .await
        .ok_or_else(|| "文件夹对话框异常:内部通道已关闭".to_string())?;

    match picked {
        Some(f) => Ok(f.as_path().map(|p| p.to_string_lossy().into_owned())),
        None => Ok(None),
    }
}

/// 在系统文件管理器中打开指定路径(文件则打开其所在文件夹,目录则直接打开)。
/// 同步命令:spawn 系统命令后立即返回,不阻塞界面。
#[tauri::command]
fn open_in_folder(path: String) -> Result<(), String> {
    let p = Path::new(&path);
    let target: PathBuf = if p.is_dir() {
        p.to_path_buf()
    } else {
        p.parent()
            .map(|d| d.to_path_buf())
            .filter(|d| !d.as_os_str().is_empty())
            .unwrap_or_else(|| PathBuf::from("."))
    };

    #[cfg(target_os = "macos")]
    let mut cmd = Command::new("open");
    #[cfg(target_os = "windows")]
    let mut cmd = Command::new("explorer");
    #[cfg(target_os = "linux")]
    let mut cmd = Command::new("xdg-open");

    cmd.arg(&target)
        .spawn()
        .map_err(|e| format!("打开文件夹失败:{}", e))?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            open_files,
            open_folder,
            open_in_folder,
            media::probe_media,
            crop::crop_media,
            thumb::make_thumbnail
        ])
        .run(tauri::generate_context!())
        .expect("媒体工具启动失败");
}
