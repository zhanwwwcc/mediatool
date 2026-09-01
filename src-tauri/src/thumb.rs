// 视频缩略图总图(contact sheet):
// 按用户填写的缩略图数量,根据视频时长自动计算取帧间隔,均匀取帧并拼成一张大图。
// 输出到源文件所在目录,文件名 = 原名-thumb.jpg(同名自动加 (1) 防覆盖)。

use serde::Serialize;
use std::path::Path;
use std::process::Command;

use crate::ffbin;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThumbResult {
    pub output_path: String,
}

/// 生成视频缩略图总图(异步,在阻塞线程池中执行,界面不卡)
/// - `input`:源文件绝对路径
/// - `count`:缩略图数量(1~200)
/// - `output_dir`:输出文件夹,None/空 表示源文件所在目录
#[tauri::command]
pub async fn make_thumbnail(
    app: tauri::AppHandle,
    input: String,
    count: u32,
    output_dir: Option<String>,
) -> Result<ThumbResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        thumb_blocking(&app, &input, count, output_dir.as_deref())
    })
    .await
    .map_err(|e| format!("缩略图任务异常退出:{}", e))?
}

fn thumb_blocking(
    app: &tauri::AppHandle,
    input: &str,
    count: u32,
    output_dir: Option<&str>,
) -> Result<ThumbResult, String> {
    /* ---------- 1. 参数校验 ---------- */
    if !(1..=200).contains(&count) {
        return Err("缩略图数量需在 1~200 之间".to_string());
    }
    let src = Path::new(input);
    if !src.is_file() {
        return Err(format!("输入文件不存在:{}", input));
    }
    let stem = match src.file_stem().and_then(|s| s.to_str()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return Err("无法从文件名获取基础名称".to_string()),
    };

    /* ---------- 2. ffprobe 读取时长与视频流 ---------- */
    let ffprobe = ffbin::resource_binary(app, "ffprobe")?;
    let out = Command::new(&ffprobe)
        .args(["-v", "error", "-print_format", "json", "-show_format", "-show_streams"])
        .arg(input)
        .output()
        .map_err(|e| format!("调用内置 ffprobe 失败:{}", e))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if err.is_empty() {
            "无法读取该文件(文件可能不是有效的媒体文件)".to_string()
        } else {
            format!("无法读取该文件:{}", err)
        });
    }
    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout)
        .map_err(|e| format!("解析 ffprobe 输出失败:{}", e))?;

    let has_video = parsed["streams"]
        .as_array()
        .map(|arr| arr.iter().any(|s| s["codec_type"].as_str() == Some("video")))
        .unwrap_or(false);
    if !has_video {
        return Err("该文件没有视频流,无法生成缩略图(纯音频文件不支持)".to_string());
    }
    let duration: f64 = parsed["format"]["duration"]
        .as_str()
        .and_then(|s| s.parse().ok())
        .filter(|d| *d > 0.0)
        .ok_or_else(|| "无法获取视频时长,不能计算缩略图间隔".to_string())?;

    /* ---------- 3. 计算取帧间隔与拼图布局 ---------- */
    // 间隔 = 时长 / 数量,保证从头到尾均匀取到 count 帧
    let interval = duration / count as f64;
    let (cols, rows) = layout_for(count);

    /* ---------- 4. 确定输出路径(用户目录或源目录,防覆盖) ---------- */
    let dir = match output_dir {
        Some(d) if !d.trim().is_empty() => {
            let p = Path::new(d.trim());
            if !p.is_dir() {
                return Err(format!("输出文件夹不存在:{}", d));
            }
            p.to_path_buf()
        }
        _ => match src.parent() {
            Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
            _ => Path::new(".").to_path_buf(),
        },
    };
    let mut out_path = dir.join(format!("{}-thumb.jpg", stem));
    let mut n = 1;
    while out_path.exists() {
        if n > 9999 {
            return Err("同名缩略图太多,无法生成不重名的输出文件".to_string());
        }
        out_path = dir.join(format!("{}-thumb ({}).jpg", stem, n));
        n += 1;
    }

    /* ---------- 5. 执行 ffmpeg(fps 均匀取帧 → 缩放 → 拼图) ---------- */
    let ffmpeg = ffbin::resource_binary(app, "ffmpeg")?;
    let vf = format!("fps=1/{:.6},scale=240:-1,tile={}x{}", interval, cols, rows);
    let cmd_out = Command::new(&ffmpeg)
        .arg("-i")
        .arg(input)
        .args(["-vf", &vf])
        .args(["-frames:v", "1", "-n"])
        .arg(&out_path)
        .output()
        .map_err(|e| format!("调用内置 ffmpeg 失败:{}", e))?;
    if !cmd_out.status.success() {
        let stderr = String::from_utf8_lossy(&cmd_out.stderr);
        return Err(format!(
            "生成缩略图失败:{}",
            useful_ffmpeg_error(&stderr)
        ));
    }
    if !out_path.exists() {
        return Err("ffmpeg 已退出,但没有生成缩略图文件".to_string());
    }

    Ok(ThumbResult {
        output_path: out_path.to_string_lossy().into_owned(),
    })
}

/// 拼图布局(列数, 行数),对应 ffmpeg tile 参数。
/// 快捷数量 6/12/20/24/36 使用用户指定的固定布局(行×列分别为 2×3、3×4、4×5、4×6、6×6);
/// 其他数量用通用公式:列 = ceil(√n),行 = ceil(n/列),保证列≥行、接近正方形。
fn layout_for(count: u32) -> (u32, u32) {
    match count {
        6 => (3, 2),
        12 => (4, 3),
        20 => (5, 4),
        24 => (6, 4),
        36 => (6, 6),
        _ => {
            let cols = ((count as f64).sqrt().ceil()).max(1.0) as u32;
            let rows = ((count as f64 / cols as f64).ceil()).max(1.0) as u32;
            (cols, rows)
        }
    }
}

/// 从 ffmpeg 的 stderr 中提取最有用的错误信息(通常在末尾)
fn useful_ffmpeg_error(stderr: &str) -> String {
    let trimmed = stderr.trim();
    if trimmed.is_empty() {
        return "未知错误(ffmpeg 未输出错误信息)".to_string();
    }
    let chars: Vec<char> = trimmed.chars().collect();
    if chars.len() <= 700 {
        return trimmed.to_string();
    }
    let tail: String = chars[chars.len() - 700..].iter().collect();
    // 从完整一行的行首开始,避免截断半个字
    match tail.find('\n') {
        Some(i) => tail[i + 1..].trim().to_string(),
        None => tail,
    }
}
