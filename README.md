# 媒体工具

一个跨平台(macOS / Windows)的极简媒体小工具:**媒体信息查看 + 快速裁剪**。
不播放视频、不渲染画面、不联网、无遥测、无更新检查,ffmpeg / ffprobe 内置于安装包,用户机器无需安装任何东西。

## 功能

### 媒体信息查看(不播放)

打开视频 / 音频文件后直接展示:

- 文件大小(如 1.2 GB)、时长(时:分:秒)、封装格式(如 MP4 / MKV / MOV)
- 视频流:分辨率、码率、帧率、编码器(如 H.264 / HEVC / VP9)
- 音频流:编码器(AAC / MP3 / FLAC…)、采样率、声道数与布局(如 2 声道 / 立体声、5.1)、码率;多音轨全部列出
- 纯音频文件同样支持

### 多文件多标签

一个窗口,每个文件一个标签页(标签显示文件名),可单独关闭,互不影响。

### 两种打开方式

1. 「打开文件」按钮:系统文件选择器,支持多选
2. 拖拽:文件直接拖进窗口自动打开;一次拖多个文件逐个开标签;已有文件时再拖入照常追加

### 快速裁剪(不重编码,秒级)

- 填起始时间(留空 = 从开头)、终止时间(必填)、输出文件名(不含扩展名)
- 时间格式容错:支持 `1:23:45`、`5:30`、`90`(纯秒数)
- 内部执行:`ffmpeg -ss <起始> -to <终止> -i <输入> -c copy <输出>`(流复制,不重编码)
- 输出保存到源文件所在目录,扩展名自动沿用源文件;同名文件自动加「 (1)」「 (2)」后缀,**绝不覆盖**
- 成功显示输出文件完整路径;失败显示 ffmpeg 的错误信息

## 下载安装

到 [Releases](https://github.com/zhanwwwcc/mediatool/releases) 下载:

| 平台 | 产物 |
| --- | --- |
| macOS Apple Silicon | `媒体工具_1.0.0_aarch64.dmg` |
| macOS Intel | `媒体工具_1.0.0_x64.dmg` |
| Windows x64 | NSIS 安装器 `.exe` |

> **macOS 注意**:应用仅做 ad-hoc 签名(未购买苹果开发者证书),首次打开若被 Gatekeeper 拦截,请右键点击应用 →「打开」,或在终端执行 `xattr -dr com.apple.quarantine /Applications/媒体工具.app`。

## 本地开发

环境要求:Node.js、Rust(stable)、Xcode Command Line Tools(macOS)。

```bash
npm install                 # 安装 Tauri CLI

# 本地运行需要先把 ffmpeg / ffprobe 放入 src-tauri/resources/(该目录不入库)
# macOS Apple Silicon 可从 https://www.osxexperts.net/ 下载 ffmpeg9arm.zip / ffprobe9arm.zip

npx tauri dev               # 开发模式运行
npx tauri build             # 本地打包(产物在 src-tauri/target/release/bundle/)
```

CI(GitHub Actions)会自动在构建时下载对应平台的 ffmpeg / ffprobe,无需提交二进制。

## 目录结构

```
├── src/                    # 前端(原生 HTML/CSS/JS,无框架)
│   ├── index.html
│   ├── style.css
│   └── main.js
├── src-tauri/              # Rust 后端
│   ├── src/
│   │   ├── lib.rs          # 入口:注册插件与命令
│   │   ├── ffbin.rs        # 内置 ffmpeg/ffprobe 的定位(绝对路径调用)
│   │   ├── media.rs        # 媒体信息探测(ffprobe → JSON → 中文键值对)
│   │   └── crop.rs         # 快速裁剪(流复制 + 防覆盖)
│   ├── resources/          # ffmpeg/ffprobe 二进制(不入库,CI/本地自行放入)
│   ├── capabilities/       # Tauri 2 权限声明
│   └── tauri.conf.json
└── .github/workflows/build.yml   # 3 平台矩阵构建 + Release
```

## 设计取舍(开发中确定的细节)

1. **ffmpeg 二进制来源**:evermeet.cx 明确不提供 Apple Silicon 原生构建,故 macOS arm64 采用 osxexperts.net(ffmpeg 9.0),macOS Intel 用 osxexperts.net 的 Intel 版(ffmpeg 8.0),Windows 用 BtbN FFmpeg-Builds(win64 gpl)。均为静态编译单文件,直接作为 Tauri resources 打进安装包。
2. **`-ss` / `-to` 语义**:已实测验证,ffmpeg 9 中 `ffmpeg -ss 10 -to 20 -i in -c copy out` 产出 10 秒片段,即 `-to` 为文件内绝对时间,与界面「起始/终止」语义一致。流复制下 seek 落在关键帧,片段起点可能有约 1 个 GOP 的偏差(符合「快速、不需精确到帧」的定位)。
3. **防覆盖**:输出前先检测同名文件并自动追加「 (1)」序号,ffmpeg 参数里再加 `-n`(绝不覆盖)双保险。
4. **ffmpeg 定位兜底**:生产环境从安装包资源目录解析;`tauri dev` 开发模式下回退到编译期记录的 `src-tauri/resources/` 源目录,方便本地调试。
5. **前端零依赖**:开启 Tauri 的 `withGlobalTauri`,通过 `window.__TAURI__` 全局 API 调用,不引入任何 npm 运行时依赖。
6. **文件对话框**在独立阻塞线程中弹出,避免阻塞界面;拖拽使用 Tauri 原生拖放事件(一次可拖入多个文件)。
7. **输出文件名校验**:不允许包含 `/ \ :` 及纯点号,防止借文件名写到其他目录。
