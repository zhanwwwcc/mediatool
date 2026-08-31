// 应用入口:注册插件与后端命令

mod crop;
mod ffbin;
mod media;

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
