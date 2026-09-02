// 媒体信息探测:调用内置 ffprobe 获取 JSON,整理为中文键值对分节返回给前端。
// 全程不播放、不渲染画面,只读取元数据。

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;

use crate::ffbin;

/* ---------- ffprobe JSON 输出的反序列化结构(字段全部容错为 Option) ---------- */

#[derive(Deserialize)]
struct FfprobeOutput {
    #[serde(default)]
    streams: Vec<StreamSection>,
    #[serde(default)]
    format: FormatSection,
}

#[derive(Deserialize, Default)]
struct FormatSection {
    #[serde(default)]
    format_name: Option<String>,
    #[serde(default)]
    duration: Option<String>,
    #[serde(default)]
    size: Option<String>,
    #[serde(default)]
    bit_rate: Option<String>,
}

#[derive(Deserialize)]
struct StreamSection {
    #[serde(default)]
    codec_type: Option<String>,
    #[serde(default)]
    codec_name: Option<String>,
    #[serde(default)]
    codec_long_name: Option<String>,
    #[serde(default)]
    width: Option<i64>,
    #[serde(default)]
    height: Option<i64>,
    #[serde(default)]
    r_frame_rate: Option<String>,
    #[serde(default)]
    avg_frame_rate: Option<String>,
    #[serde(default)]
    bit_rate: Option<String>,
    #[serde(default)]
    sample_rate: Option<String>,
    #[serde(default)]
    channels: Option<i64>,
    #[serde(default)]
    channel_layout: Option<String>,
}

/* ---------- 返回给前端的结构 ---------- */

#[derive(Serialize, Clone)]
pub struct InfoItem {
    pub label: String,
    pub value: String,
}

#[derive(Serialize, Clone)]
pub struct InfoSection {
    pub title: String,
    pub items: Vec<InfoItem>,
}

/// 探测媒体文件信息(异步命令,在阻塞线程池中执行,界面不卡)
#[tauri::command]
pub async fn probe_media(
    app: tauri::AppHandle,
    path: String,
) -> Result<Vec<InfoSection>, String> {
    tauri::async_runtime::spawn_blocking(move || probe_blocking(&app, &path))
        .await
        .map_err(|e| format!("探测任务异常退出:{}", e))?
}

fn probe_blocking(app: &tauri::AppHandle, path: &str) -> Result<Vec<InfoSection>, String> {
    let p = Path::new(path);
    if p.is_dir() {
        return Err(format!("这是一个文件夹,请拖入或选择媒体文件:{}", path));
    }
    if !p.exists() {
        return Err(format!("文件不存在:{}", path));
    }

    let ffprobe = ffbin::resource_binary(app, "ffprobe")?;

    // 约定用法:错误级别静默 + JSON 输出 + 容器与流信息
    let out = ffbin::prepare(Command::new(&ffprobe))
        .args(["-v", "error", "-print_format", "json", "-show_format", "-show_streams"])
        .arg(path)
        .output()
        .map_err(|e| format!("调用内置 ffprobe 失败:{}", e))?;

    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if err.is_empty() {
            "ffprobe 返回错误且没有输出错误信息(文件可能不是有效的媒体文件)".to_string()
        } else {
            format!("无法读取该文件:{}", err)
        });
    }

    let parsed: FfprobeOutput = serde_json::from_slice(&out.stdout)
        .map_err(|e| format!("解析 ffprobe 输出失败:{}", e))?;

    build_sections(path, &parsed)
}

/* ---------- 组装展示分节 ---------- */

fn build_sections(path: &str, parsed: &FfprobeOutput) -> Result<Vec<InfoSection>, String> {
    let videos: Vec<&StreamSection> = parsed
        .streams
        .iter()
        .filter(|s| s.codec_type.as_deref() == Some("video"))
        .collect();
    let audios: Vec<&StreamSection> = parsed
        .streams
        .iter()
        .filter(|s| s.codec_type.as_deref() == Some("audio"))
        .collect();

    if videos.is_empty() && audios.is_empty() {
        return Err("该文件中没有可识别的视频流或音频流".to_string());
    }

    let mut sections: Vec<InfoSection> = Vec::new();

    // 一、文件信息
    let mut items: Vec<InfoItem> = Vec::new();
    if let Some(v) = parsed.format.size.as_deref().and_then(parse_f64) {
        items.push(item("文件大小", human_size(v)));
    }
    if let Some(v) = parsed.format.duration.as_deref().and_then(parse_f64) {
        items.push(item("时长", fmt_duration(v)));
    }
    items.push(item(
        "封装格式",
        container_name(path, parsed.format.format_name.as_deref()),
    ));
    if let Some(raw) = &parsed.format.format_name {
        items.push(item("原始格式标签", raw.clone()));
    }
    // 纯音频文件:补充总码率
    if videos.is_empty() {
        if let Some(v) = parsed.format.bit_rate.as_deref().and_then(parse_f64) {
            items.push(item("总码率", fmt_bitrate(v)));
        }
    }
    sections.push(InfoSection {
        title: "文件信息".to_string(),
        items,
    });

    // 二、视频流(可能多条,全部列出)
    for (i, s) in videos.iter().enumerate() {
        let title = if videos.len() > 1 {
            format!("视频流 {}", i + 1)
        } else {
            "视频流".to_string()
        };
        let mut items: Vec<InfoItem> = Vec::new();
        if let (Some(w), Some(h)) = (s.width, s.height) {
            items.push(item("分辨率", format!("{} × {}", w, h)));
        }
        if let Some(fps) = frame_rate(s) {
            items.push(item("帧率", fps));
        }
        if let Some(v) = s.bit_rate.as_deref().and_then(parse_f64) {
            items.push(item("码率", fmt_bitrate(v)));
        }
        items.push(item("编码器", codec_display(s)));
        sections.push(InfoSection { title, items });
    }

    // 三、音频流(可能多条音轨,全部列出)
    for (i, s) in audios.iter().enumerate() {
        let title = if audios.len() > 1 {
            format!("音频流 {}", i + 1)
        } else {
            "音频流".to_string()
        };
        let mut items: Vec<InfoItem> = Vec::new();
        items.push(item("编码器", codec_display(s)));
        if let Some(v) = s.sample_rate.as_deref() {
            items.push(item("采样率", format!("{} Hz", v)));
        }
        items.push(item("声道", channel_display(s)));
        if let Some(v) = s.bit_rate.as_deref().and_then(parse_f64) {
            items.push(item("码率", fmt_bitrate(v)));
        }
        sections.push(InfoSection { title, items });
    }

    Ok(sections)
}

/* ---------- 格式化辅助函数 ---------- */

fn item(label: &str, value: String) -> InfoItem {
    InfoItem {
        label: label.to_string(),
        value,
    }
}

/// 字符串安全转 f64
fn parse_f64(s: &str) -> Option<f64> {
    s.trim().parse::<f64>().ok()
}

/// 字节数 → 人类可读(如 1.2 GB)
fn human_size(bytes: f64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{} B", bytes as u64)
    } else {
        format!("{:.1} {}", v, UNITS[u])
    }
}

/// 秒 → 时:分:秒(如 1:23:45;不足 1 秒按 1 秒显示,避免出现 0:00:00)
fn fmt_duration(secs: f64) -> String {
    let total = (secs.max(0.0).round() as u64).max(if secs > 0.0 { 1 } else { 0 });
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    format!("{}:{:02}:{:02}", h, m, s)
}

/// 码率 bps → 人类可读(如 8.5 Mbps / 320 kbps)
fn fmt_bitrate(bps: f64) -> String {
    if bps >= 1_000_000.0 {
        format!("{:.2} Mbps", bps / 1_000_000.0)
    } else if bps >= 1000.0 {
        format!("{:.0} kbps", bps / 1000.0)
    } else {
        format!("{:.0} bps", bps)
    }
}

/// 分数字符串("30000/1001")→ 浮点值
fn parse_ratio(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.is_empty() || s == "0/0" {
        return None;
    }
    if let Some((a, b)) = s.split_once('/') {
        let a: f64 = a.trim().parse().ok()?;
        let b: f64 = b.trim().parse().ok()?;
        if b > 0.0 {
            return Some(a / b);
        }
        None
    } else {
        s.parse().ok()
    }
}

/// 帧率:优先 r_frame_rate,无效时回退 avg_frame_rate
fn frame_rate(s: &StreamSection) -> Option<String> {
    let v = s
        .r_frame_rate
        .as_deref()
        .and_then(parse_ratio)
        .or_else(|| s.avg_frame_rate.as_deref().and_then(parse_ratio))?;
    if v <= 0.0 {
        return None;
    }
    if (v - v.round()).abs() < 0.05 {
        Some(format!("{:.0} fps", v))
    } else {
        Some(format!("{:.2} fps", v))
    }
}

/// 编码器名称:常见 codec_name 转为惯用写法,其余大写展示
fn codec_display(s: &StreamSection) -> String {
    let name = match s.codec_name.as_deref() {
        None => return "未知".to_string(),
        Some(n) => n,
    };
    let mapped = match name {
        "h264" => "H.264",
        "h265" | "hevc" => "HEVC",
        "vp8" => "VP8",
        "vp9" => "VP9",
        "av1" => "AV1",
        "mpeg4" => "MPEG-4",
        "mpeg2video" => "MPEG-2",
        "mpeg1video" => "MPEG-1",
        "theora" => "Theora",
        "prores" => "ProRes",
        "dnxhr" => "DNxHR",
        "aac" => "AAC",
        "mp3" => "MP3",
        "flac" => "FLAC",
        "alac" => "ALAC",
        "opus" => "Opus",
        "vorbis" => "Vorbis",
        "pcm_s16le" => "PCM 16-bit",
        "pcm_s24le" => "PCM 24-bit",
        "pcm_f32le" => "PCM 32-bit float",
        "ac3" => "AC-3",
        "eac3" => "E-AC-3",
        "dts" => "DTS",
        "truehd" => "TrueHD",
        "amr_nb" => "AMR-NB",
        other => return other.to_uppercase(),
    };
    // 有长名称时补充在括号里(如 H.264(H.264 / AVC / MPEG-4 AVC))
    if let Some(long) = s.codec_long_name.as_deref() {
        let long = long.trim();
        if !long.is_empty() {
            return format!("{}({})", mapped, long);
        }
    }
    mapped.to_string()
}

/// 声道数与布局(如「2 声道 / 立体声」「6 声道 / 5.1」)
fn channel_display(s: &StreamSection) -> String {
    let layout = match s.channel_layout.as_deref() {
        None => None,
        Some(l) => match l.trim() {
            "mono" => Some("单声道".to_string()),
            "stereo" => Some("立体声".to_string()),
            "quad" => Some("四声道".to_string()),
            "quad(side)" => Some("四声道".to_string()),
            "5.1" | "5.1(side)" | "6.1" | "7.1" | "7.1(wide)" => Some(l.to_string()),
            other if !other.is_empty() => Some(other.to_string()),
            _ => None,
        },
    };
    match (s.channels, layout) {
        (Some(c), Some(l)) => format!("{} 声道 / {}", c, l),
        (Some(c), None) => format!("{} 声道", c),
        (None, Some(l)) => l,
        (None, None) => "未知".to_string(),
    }
}

/// 封装格式:优先用常见扩展名判断(避免 mov,mp4,… 被显示成 MOV),否则取格式标签首段
fn container_name(path: &str, format_name: Option<&str>) -> String {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.trim().to_ascii_lowercase())
        .unwrap_or_default();
    if ext.len() <= 5 && ext.chars().all(|c| c.is_ascii_alphanumeric()) && !ext.is_empty() {
        return ext.to_ascii_uppercase();
    }
    format_name
        .and_then(|f| f.split(',').next())
        .map(|f| f.trim().to_ascii_uppercase())
        .unwrap_or_else(|| "未知".to_string())
}
