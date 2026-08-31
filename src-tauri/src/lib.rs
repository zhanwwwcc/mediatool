// 应用入口:注册插件与后端命令

mod crop;
mod ffbin;
mod media;

use tauri_plugin_dialog::DialogExt;

/// 打开系统文件选择器(支持多选),返回所选文件的绝对路径列表。
/// 对话框在独立阻塞线程中弹出,避免卡住界面。
#[tauri::command]
async fn open_files(app: tauri::AppHandle) -> Result<Vec<String>, String> {
    let handle = app.clone();
    let picked = tauri::async_runtime::spawn_blocking(move || {
        handle
            .dialog()
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
            .blocking_pick_files()
    })
    .await
    .map_err(|e| format!("打开文件选择器失败:{}", e))?;

    match picked {
        Some(files) => Ok(files
            .into_iter()
            .filter_map(|f| f.as_path().map(|p| p.to_string_lossy().into_owned()))
            .collect()),
        // 用户取消选择不算错误
        None => Ok(Vec::new()),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            open_files,
            media::probe_media,
            crop::crop_media
        ])
        .run(tauri::generate_context!())
        .expect("媒体工具启动失败");
}
