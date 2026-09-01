// 快速裁剪:ffmpeg 流复制(-c copy),不重编码,秒级完成。
// 命令形态与任务约定一致:-ss / -to 放在 -i 之前实现快速 seek。
//   ffmpeg -ss <起始> -to <终止> -i "<输入文件>" -c copy "<输出文件>"
// 输出保存到源文件所在目录,扩展名沿用源文件;同名文件自动加「 (1)」后缀,禁止覆盖。

use serde::Serialize;
use std::path::Path;
use std::process::Command;

use crate::ffbin;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CropResult {
    pub output_path: String,
}

/// 裁剪命令(异步,在阻塞线程池中执行)
/// - `input`:源文件绝对路径
/// - `start`:起始秒数,None 表示从文件开头开始
/// - `end`:终止秒数,None 表示裁剪到文件结尾
/// - `output_name`:用户填写的输出文件名(不含扩展名,为空时由前端自动生成 原名-cut)
/// - `output_dir`:输出文件夹,None/空 表示源文件所在目录
#[tauri::command]
pub async fn crop_media(
    app: tauri::AppHandle,
    input: String,
    start: Option<f64>,
    end: Option<f64>,
    output_name: String,
    output_dir: Option<String>,
) -> Result<CropResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crop_blocking(&app, &input, start, end, &output_name, output_dir.as_deref())
    })
    .await
    .map_err(|e| format!("裁剪任务异常退出:{}", e))?
}

fn crop_blocking(
    app: &tauri::AppHandle,
    input: &str,
    start: Option<f64>,
    end: Option<f64>,
    output_name: &str,
    output_dir: Option<&str>,
) -> Result<CropResult, String> {
    /* ---------- 1. 参数校验 ---------- */
    let name = validate_output_name(output_name)?;
    if let Some(e) = end {
        if !e.is_finite() || e <= 0.0 {
            return Err("终止时间必须大于 0 秒".to_string());
        }
        if let Some(s) = start {
            if s >= e {
                return Err("起始时间必须早于终止时间".to_string());
            }
        }
    }
    if let Some(s) = start {
        if s < 0.0 {
            return Err("起始时间不能为负数".to_string());
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
    let mut cmd = Command::new(&ffmpeg);
    if let Some(s) = start {
        cmd.arg("-ss").arg(fmt_secs(s));
    }
    // 终止时间留空时省略 -to,表示裁剪到文件结尾
    if let Some(e) = end {
        cmd.arg("-to").arg(fmt_secs(e));
    }
    cmd.arg("-i").arg(input);
    // -c copy 流复制;-n 表示绝不覆盖已存在的文件(与上面去重逻辑双保险)
    cmd.args(["-c", "copy", "-n"]).arg(&out_path);

    let out = cmd
        .output()
        .map_err(|e| format!("调用内置 ffmpeg 失败:{}", e))?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!("裁剪失败:{}", useful_ffmpeg_error(&stderr)));
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
