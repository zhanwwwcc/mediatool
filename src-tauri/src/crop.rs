// 快速裁剪:ffmpeg 流复制(-c copy),不重编码,秒级完成。
// 支持多片段:每个片段一段起始/终止时间,按用户添加的顺序切出后无损合并成一个文件。
// 单片段直接 -ss/-to -c copy 输出;多片段先逐段流复制切出临时文件,再用 concat demuxer 无损拼接。
//   ffmpeg -ss <起始> -to <终止> -i "<输入文件>" -c copy "<输出文件>"
// 输出保存到指定目录(默认源文件所在目录),扩展名沿用源文件;同名文件自动加「 (1)」后缀,禁止覆盖。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::ffbin;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CropResult {
    pub output_path: String,
}

/// 单个裁剪片段:起始/终止秒数,None 表示留空(开头/结尾)
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CropSegment {
    pub start: Option<f64>,
    pub end: Option<f64>,
}

/// 裁剪命令(异步,在阻塞线程池中执行)
/// - `input`:源文件绝对路径
/// - `segments`:按顺序排列的裁剪片段(至少 1 个),依次切出后合并
/// - `output_name`:输出文件名(不含扩展名,为空时由前端自动生成 原名-cut)
/// - `output_dir`:输出文件夹,None/空 表示源文件所在目录
#[tauri::command]
pub async fn crop_media(
    app: tauri::AppHandle,
    input: String,
    segments: Vec<CropSegment>,
    output_name: String,
    output_dir: Option<String>,
) -> Result<CropResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crop_blocking(&app, &input, &segments, &output_name, output_dir.as_deref())
    })
    .await
    .map_err(|e| format!("裁剪任务异常退出:{}", e))?
}

/// 临时目录守卫:函数返回时自动删除整个临时目录
struct TempCleanup(PathBuf);
impl Drop for TempCleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn crop_blocking(
    app: &tauri::AppHandle,
    input: &str,
    segments: &[CropSegment],
    output_name: &str,
    output_dir: Option<&str>,
) -> Result<CropResult, String> {
    /* ---------- 1. 参数校验 ---------- */
    let name = validate_output_name(output_name)?;
    if segments.is_empty() {
        return Err("至少需要一个裁剪片段".to_string());
    }
    for (i, seg) in segments.iter().enumerate() {
        if seg.start.is_none() && seg.end.is_none() {
            return Err(format!("片段 {} 的起始和终止时间不能同时为空", i + 1));
        }
        if let Some(e) = seg.end {
            if !e.is_finite() || e <= 0.0 {
                return Err(format!("片段 {} 的终止时间必须大于 0 秒", i + 1));
            }
            if let Some(s) = seg.start {
                if s >= e {
                    return Err(format!("片段 {} 的起始时间必须早于终止时间", i + 1));
                }
            }
        }
        if let Some(s) = seg.start {
            if s < 0.0 {
                return Err(format!("片段 {} 的起始时间不能为负数", i + 1));
            }
        }
    }

    /* ---------- 2. 确定输出路径 ---------- */
    let src = Path::new(input);
    if !src.is_file() {
        return Err(format!("输入文件不存在:{}", input));
    }
    let ext = match src.extension().and_then(|e| e.to_str()) {
        Some(e) if !e.is_empty() => e.to_string(),
        _ => return Err("源文件没有扩展名,无法确定输出格式".to_string()),
    };
    // 输出目录:用户指定时用指定目录,否则用源文件所在目录
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
    let ext_part = format!(".{}", ext);
    let mut out_path = dir.join(format!("{}{}", name, ext_part));
    // 同名不覆盖:自动追加「 (1)」「 (2)」…
    let mut n = 1;
    while out_path.exists() {
        if n > 9999 {
            return Err("同名文件太多,无法生成不重名的输出文件".to_string());
        }
        out_path = dir.join(format!("{} ({}){}", name, n, ext_part));
        n += 1;
    }

    /* ---------- 3. 执行 ffmpeg(流复制,不重编码) ---------- */
    let ffmpeg = ffbin::resource_binary(app, "ffmpeg")?;

    if segments.len() == 1 {
        // 单片段:直接切出
        let seg = &segments[0];
        let mut cmd = ffbin::prepare(Command::new(&ffmpeg));
        if let Some(s) = seg.start {
            cmd.arg("-ss").arg(fmt_secs(s));
        }
        if let Some(e) = seg.end {
            cmd.arg("-to").arg(fmt_secs(e));
        }
        cmd.arg("-i").arg(input);
        cmd.args(["-c", "copy", "-n"]).arg(&out_path);
        let out = cmd.output().map_err(|e| format!("调用内置 ffmpeg 失败:{}", e))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(format!("裁剪失败:{}", useful_ffmpeg_error(&stderr)));
        }
    } else {
        // 多片段:逐段流复制切出临时文件,再 concat demuxer 无损合并
        let tmp = std::env::temp_dir().join(format!("mediatool-crop-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).map_err(|e| format!("创建临时目录失败:{}", e))?;
        let _guard = TempCleanup(tmp.clone()); // 函数返回时自动清理

        let mut list = String::new();
        for (i, seg) in segments.iter().enumerate() {
            let seg_path = tmp.join(format!("seg_{}.{}", i, ext));
            let mut cmd = ffbin::prepare(Command::new(&ffmpeg));
            if let Some(s) = seg.start {
                cmd.arg("-ss").arg(fmt_secs(s));
            }
            if let Some(e) = seg.end {
                cmd.arg("-to").arg(fmt_secs(e));
            }
            cmd.arg("-i").arg(input);
            cmd.args(["-c", "copy", "-y"]).arg(&seg_path);
            let out = cmd.output().map_err(|e| format!("调用内置 ffmpeg 失败:{}", e))?;
            if !out.status.success() {
                let stderr = String::from_utf8_lossy(&out.stderr);
                return Err(format!("片段 {} 裁剪失败:{}", i + 1, useful_ffmpeg_error(&stderr)));
            }
            list.push_str(&format!("file '{}'\n", seg_path.display()));
        }

        let list_path = tmp.join("list.txt");
        std::fs::write(&list_path, &list).map_err(|e| format!("写入合并清单失败:{}", e))?;
        let mut cmd = ffbin::prepare(Command::new(&ffmpeg));
        cmd.args(["-f", "concat", "-safe", "0", "-i"]).arg(&list_path);
        cmd.args(["-c", "copy", "-y"]).arg(&out_path);
        let out = cmd.output().map_err(|e| format!("调用内置 ffmpeg 失败:{}", e))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(format!("片段合并失败:{}", useful_ffmpeg_error(&stderr)));
        }
    }

    if !out_path.exists() {
        return Err("ffmpeg 已退出,但没有生成输出文件(可能是时间范围超出文件时长)".to_string());
    }

    Ok(CropResult {
        output_path: out_path.to_string_lossy().into_owned(),
    })
}

/* ---------- 辅助函数 ---------- */

/// 校验并规整输出文件名(不含扩展名):
/// 不允许为空、不允许包含路径分隔符,避免用户借此写到其他目录
fn validate_output_name(name: &str) -> Result<String, String> {
    let n = name.trim().trim_end_matches(['.', ' ']);
    if n.is_empty() {
        return Err("请填写输出文件名(不含扩展名)".to_string());
    }
    if n == "." || n == ".." {
        return Err("输出文件名不能是 . 或 ..".to_string());
    }
    if n.contains('/') || n.contains('\\') || n.contains(':') {
        return Err("输出文件名不能包含 / \\ : 字符".to_string());
    }
    if n.chars().count() > 200 {
        return Err("输出文件名过长".to_string());
    }
    Ok(n.to_string())
}

/// 秒数 → ffmpeg 可接受的字符串(毫秒精度)
fn fmt_secs(v: f64) -> String {
    format!("{:.3}", v.max(0.0))
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
