/* 媒体工具 - 前端逻辑(原生 JS,无框架)
 * 依赖 Tauri 注入的全局 API:window.__TAURI__(见 tauri.conf.json 的 withGlobalTauri)
 * 原则:任何失败都必须让用户看得见,绝不允许静默无反应。
 */

/* ---------- 全局错误兜底:JS 任何异常都显示为顶部红色横幅 ---------- */

function showFatalBanner(msg) {
  let el = document.getElementById('fatal-banner');
  if (!el) {
    el = document.createElement('div');
    el.id = 'fatal-banner';
    document.body.prepend(el);
  }
  el.textContent = '运行异常:' + msg + '(请把这段文字反馈给开发者)';
  el.style.display = 'block';
}

window.addEventListener('error', (e) => {
  showFatalBanner(e.message || '未知脚本错误');
});
window.addEventListener('unhandledrejection', (e) => {
  showFatalBanner(String(e.reason || '未知异步错误'));
});

/* ---------- Tauri API 检测 ---------- */

function getInvoke() {
  const t = window.__TAURI__;
  if (t && t.core && typeof t.core.invoke === 'function') return t.core.invoke;
  return null;
}

function getWebview() {
  const t = window.__TAURI__;
  if (t && t.webview && typeof t.webview.getCurrentWebview === 'function') {
    return t.webview.getCurrentWebview();
  }
  return null;
}

/* ---------- DOM ---------- */

const tabsEl = document.getElementById('tabs');
const contentEl = document.getElementById('content');
const emptyEl = document.getElementById('empty-state');
const openBtn = document.getElementById('btn-open');
const filepathEl = document.getElementById('filepath');
const panelBarEl = document.getElementById('panel-bar');
const cropPanelBtn = document.getElementById('btn-crop-panel');
const thumbPanelBtn = document.getElementById('btn-thumb-panel');

/** 标签页数组,每个元素对应一个打开的文件 */
const tabs = [];
let tabSeq = 0;

/* ---------- 小工具 ---------- */

function escapeHtml(s) {
  return String(s).replace(/[&<>"']/g, (c) => ({
    '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;'
  }[c]));
}

function fileNameOf(path) {
  const i = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'));
  return i >= 0 ? path.slice(i + 1) : path;
}

/** 文件名去扩展名(如 base.mp4 → base) */
function stemName(path) {
  const base = fileNameOf(path);
  const i = base.lastIndexOf('.');
  return i > 0 ? base.slice(0, i) : base;
}

/**
 * 容错时间解析:支持 时:分:秒(1:23:45)、分:秒(5:30)、纯秒数(90 或 90.5)
 * @returns {number|null} 秒数;空串返回 null;格式非法返回 NaN
 */
function parseTime(str) {
  if (str == null) return null;
  str = String(str).trim();
  if (str === '') return null;
  if (/^\d+(\.\d+)?$/.test(str)) return parseFloat(str);
  const m = str.match(/^(\d{1,4}):(\d{1,2})(?::(\d{1,2}(?:\.\d+)?))?$/);
  if (!m) return NaN;
  const a = parseInt(m[1], 10);
  const b = parseInt(m[2], 10);
  if (m[3] === undefined) return a * 60 + b;
  return a * 3600 + b * 60 + parseFloat(m[3]);
}

/* ---------- 标签页管理 ---------- */

/** 打开(或聚焦已打开的)文件:每个文件一个标签页,互相独立 */
async function openFile(path) {
  if (!path || typeof path !== 'string') return;

  const existing = tabs.find((t) => t.path === path);
  if (existing) { activateTab(existing.id); return; }

  const tab = { id: ++tabSeq, path, name: fileNameOf(path) };

  try {
    tabs.push(tab);

    // 标签栏中的按钮
    tab.tabEl = document.createElement('div');
    tab.tabEl.className = 'tab';
    tab.tabEl.innerHTML = `<span class="tab-name" title="${escapeHtml(path)}">${escapeHtml(tab.name)}</span><span class="tab-close" title="关闭标签">✕</span>`;
    tab.tabEl.addEventListener('click', (e) => {
      if (e.target.classList.contains('tab-close')) { closeTab(tab.id); return; }
      activateTab(tab.id);
    });
    tabsEl.appendChild(tab.tabEl);
    tabsEl.hidden = false;
    emptyEl.hidden = true;

    // 标签页主体:信息区(默认占满) + 裁剪区/缩略图区(默认隐藏,由左下角按钮展开)
    tab.pageEl = document.createElement('div');
    tab.pageEl.className = 'tab-page';
    tab.pageEl.innerHTML = `
      <div class="info-area"><div class="info-loading">正在读取媒体信息…</div></div>
      <div class="crop-area" hidden>
        <h2>快速裁剪(流复制,不重编码)</h2>
        <div class="crop-hint">时间格式支持 时:分:秒(如 1:23:45)、分:秒(如 5:30)或纯秒数(如 90);起始、终止均可留空(默认从开头裁剪到文件结尾)</div>
        <div class="form-row">
          <label for="start-${tab.id}">起始时间</label>
          <input type="text" id="start-${tab.id}" class="crop-start" placeholder="00:00:00(可留空)">
          <span class="inline-hint">默认从文件开头</span>
        </div>
        <div class="form-row">
          <label for="end-${tab.id}">终止时间</label>
          <input type="text" id="end-${tab.id}" class="crop-end" placeholder="00:00:00(可留空)">
          <span class="inline-hint">默认到文件结尾</span>
        </div>
        <div class="form-row">
          <label for="name-${tab.id}">输出文件名</label>
          <input type="text" id="name-${tab.id}" class="crop-name" placeholder="留空自动:原名-cut">
          <span class="inline-hint">扩展名自动沿用源文件</span>
        </div>
        <div class="form-row">
          <label for="crop-dir-${tab.id}">输出文件夹</label>
          <input type="text" id="crop-dir-${tab.id}" class="crop-dir" placeholder="留空 = 源文件所在目录" readonly>
          <button type="button" class="dir-btn" data-target="crop-dir-${tab.id}">选择…</button>
        </div>
        <div class="form-actions">
          <button type="button" class="crop-btn">裁剪</button>
          <span class="crop-status"></span>
        </div>
      </div>
      <div class="thumb-area" hidden>
        <h2>视频缩略图</h2>
        <div class="thumb-hint">填写缩略图数量,程序按视频时长自动均分取帧,生成一张总图(纯音频文件不支持)</div>
        <div class="form-row">
          <label for="thumb-count-${tab.id}">缩略图数量</label>
          <input type="number" id="thumb-count-${tab.id}" class="thumb-count" min="1" max="200" placeholder="如 12">
          <div class="thumb-quick" role="group" aria-label="快捷数量">
            <button type="button" data-count="6" title="横 2 × 竖 3">6</button>
            <button type="button" data-count="12" title="横 3 × 竖 4">12</button>
            <button type="button" data-count="20" title="横 4 × 竖 5">20</button>
            <button type="button" data-count="24" title="横 4 × 竖 6">24</button>
            <button type="button" data-count="36" title="横 6 × 竖 6">36</button>
          </div>
          <span class="inline-hint">点一下填入</span>
        </div>
        <div class="form-row">
          <label for="thumb-dir-${tab.id}">输出文件夹</label>
          <input type="text" id="thumb-dir-${tab.id}" class="thumb-dir" placeholder="留空 = 源文件所在目录" readonly>
          <button type="button" class="dir-btn" data-target="thumb-dir-${tab.id}">选择…</button>
        </div>
        <div class="form-actions">
          <button type="button" class="thumb-btn">生成缩略图</button>
          <span class="thumb-status"></span>
        </div>
      </div>`;
    contentEl.appendChild(tab.pageEl);

    tab.infoEl = tab.pageEl.querySelector('.info-area');
    tab.cropAreaEl = tab.pageEl.querySelector('.crop-area');
    tab.thumbAreaEl = tab.pageEl.querySelector('.thumb-area');
    tab.statusEl = tab.pageEl.querySelector('.crop-status');
    tab.btnEl = tab.pageEl.querySelector('.crop-btn');
    tab.startEl = tab.pageEl.querySelector('.crop-start');
    tab.endEl = tab.pageEl.querySelector('.crop-end');
    tab.nameEl = tab.pageEl.querySelector('.crop-name');
    tab.dirEl = tab.pageEl.querySelector('.crop-dir');

    tab.btnEl.addEventListener('click', () => runCrop(tab));

    // 缩略图区:快捷数量按钮、输入框、生成按钮
    tab.thumbCountEl = tab.pageEl.querySelector('.thumb-count');
    tab.thumbBtnEl = tab.pageEl.querySelector('.thumb-btn');
    tab.thumbStatusEl = tab.pageEl.querySelector('.thumb-status');
    tab.thumbDirEl = tab.pageEl.querySelector('.thumb-dir');
    tab.pageEl.querySelectorAll('.thumb-quick button').forEach((btn) => {
      btn.addEventListener('click', () => {
        tab.thumbCountEl.value = btn.dataset.count;
        tab.thumbCountEl.focus();
      });
    });
    tab.thumbBtnEl.addEventListener('click', () => runThumbnail(tab));

    // 输出文件夹「选择…」按钮:两个板块共用,按 data-target 找目标输入框
    tab.pageEl.querySelectorAll('.dir-btn').forEach((btn) => {
      btn.addEventListener('click', async () => {
        const target = document.getElementById(btn.dataset.target);
        const invoke = getInvoke();
        if (!invoke || !target) return;
        try {
          const dir = await invoke('open_folder');
          if (dir) target.value = dir;
        } catch (err) {
          alert('选择文件夹失败:' + err);
        }
      });
    });

    activateTab(tab.id);
  } catch (err) {
    showFatalBanner('创建标签页失败:' + err);
    return;
  }

  // 异步探测媒体信息,失败时在信息区显示错误(不影响裁剪区展示)
  try {
    const invoke = getInvoke();
    if (!invoke) throw new Error('Tauri API 未注入,无法读取媒体信息');
    const sections = await invoke('probe_media', { path });
    if (!tabs.includes(tab)) return; // 探测期间标签可能已被关闭
    if (!Array.isArray(sections)) throw new Error('后端返回了意外的数据格式');
    renderInfo(tab, sections);
  } catch (err) {
    if (!tabs.includes(tab)) return;
    tab.infoEl.innerHTML = `<div class="info-error">${escapeHtml(String(err))}</div>`;
  }
}

/** 切换到指定标签页 */
function activateTab(id) {
  let active = null;
  tabs.forEach((t) => {
    const isActive = t.id === id;
    t.tabEl.classList.toggle('active', isActive);
    t.pageEl.classList.toggle('active', isActive);
    if (isActive) active = t;
  });
  // 同步更新全局路径栏与底部功能按钮
  if (active) {
    filepathEl.textContent = `文件:${active.path}`;
    filepathEl.title = active.path;
    filepathEl.hidden = false;
    panelBarEl.hidden = false;
    syncPanelButtons(active);
  } else {
    filepathEl.hidden = true;
    panelBarEl.hidden = true;
  }
}

/** 让底部两个按钮的高亮状态与当前标签页的面板开合一致 */
function syncPanelButtons(tab) {
  cropPanelBtn.classList.toggle('active', tab && !tab.cropAreaEl.hidden);
  thumbPanelBtn.classList.toggle('active', tab && !tab.thumbAreaEl.hidden);
}

/** 当前激活标签页 */
function activeTab() {
  for (const t of tabs) {
    if (t.tabEl.classList.contains('active')) return t;
  }
  return null;
}

/* ---------- 底部功能按钮:展开/收起裁剪、缩略图面板 ---------- */

cropPanelBtn.addEventListener('click', () => {
  const tab = activeTab();
  if (!tab) return;
  tab.cropAreaEl.hidden = !tab.cropAreaEl.hidden;
  syncPanelButtons(tab);
});

thumbPanelBtn.addEventListener('click', () => {
  const tab = activeTab();
  if (!tab) return;
  tab.thumbAreaEl.hidden = !tab.thumbAreaEl.hidden;
  syncPanelButtons(tab);
});

/** 关闭标签页 */
function closeTab(id) {
  const idx = tabs.findIndex((t) => t.id === id);
  if (idx < 0) return;
  const wasActive = tabs[idx].tabEl.classList.contains('active');
  tabs[idx].tabEl.remove();
  tabs[idx].pageEl.remove();
  tabs.splice(idx, 1);
  if (tabs.length === 0) {
    tabsEl.hidden = true;
    emptyEl.hidden = false;
    filepathEl.hidden = true;
    panelBarEl.hidden = true;
  } else if (wasActive) {
    activateTab(tabs[Math.max(0, idx - 1)].id);
  }
}

/* ---------- 信息渲染 ---------- */

function renderInfo(tab, sections) {
  const html = sections.map((sec) => `
    <section class="info-section">
      <h3>${escapeHtml(sec.title)}</h3>
      <dl class="kv">
        ${(sec.items || []).map((it) =>
          `<div class="kv-row"><dt>${escapeHtml(it.label)}</dt><dd>${escapeHtml(it.value)}</dd></div>`
        ).join('')}
      </dl>
    </section>`).join('');
  tab.infoEl.innerHTML = html;
}

/* ---------- 裁剪 ---------- */

async function runCrop(tab) {
  // 1. 解析并校验时间输入(前端先挡一道,Rust 端还会再校验)
  const start = parseTime(tab.startEl.value);
  const end = parseTime(tab.endEl.value);

  if (isNaN(start) || (start !== null && start < 0)) {
    setStatus(tab, 'error', '起始时间格式不对:支持 时:分:秒(1:23:45)、分:秒(5:30)或纯秒数(90)');
    return;
  }
  if (end !== null && (isNaN(end) || end <= 0)) {
    setStatus(tab, 'error', '终止时间需大于 0(留空表示裁剪到文件结尾)');
    return;
  }
  if (start !== null && end !== null && start >= end) {
    setStatus(tab, 'error', '起始时间必须早于终止时间');
    return;
  }

  // 2. 输出文件名(不含扩展名,扩展名由 Rust 端沿用源文件;留空自动用「原名-cut」)
  let outName = tab.nameEl.value.trim();
  if (!outName) {
    outName = stemName(tab.path) + '-cut';
  }
  if (/[\\/:]/.test(outName)) {
    setStatus(tab, 'error', '输出文件名不能包含 / \\ : 等字符');
    return;
  }

  // 3. 调用 Rust 命令执行裁剪(阻塞在后端线程,界面不卡)
  const invoke = getInvoke();
  if (!invoke) {
    setStatus(tab, 'error', 'Tauri API 未注入,无法执行裁剪');
    return;
  }
  tab.btnEl.disabled = true;
  setStatus(tab, 'pending', '正在裁剪(流复制,通常几秒完成)…');
  try {
    const result = await invoke('crop_media', {
      input: tab.path,
      start: start,
      end: end,
      outputName: outName,
      outputDir: tab.dirEl.value.trim() || null,
    });
    setStatus(tab, 'success', `裁剪完成:${result.outputPath}`);
  } catch (err) {
    setStatus(tab, 'error', String(err));
  } finally {
    tab.btnEl.disabled = false;
  }
}

function setStatus(tab, kind, text) {
  tab.statusEl.className = `crop-status ${kind}`;
  tab.statusEl.textContent = text;
}

/* ---------- 视频缩略图 ---------- */

async function runThumbnail(tab) {
  // 1. 校验数量(1~200 的整数,允许为空提示)
  const raw = tab.thumbCountEl.value.trim();
  if (raw === '') {
    setThumbStatus(tab, 'error', '请填写缩略图数量,或点右侧快捷按钮(6/12/20/24/36)');
    return;
  }
  const count = parseInt(raw, 10);
  if (!Number.isInteger(count) || String(count) !== raw || count < 1 || count > 200) {
    setThumbStatus(tab, 'error', '缩略图数量需为 1~200 的整数');
    return;
  }

  // 2. 调用 Rust 命令生成总图
  const invoke = getInvoke();
  if (!invoke) {
    setThumbStatus(tab, 'error', 'Tauri API 未注入,无法生成缩略图');
    return;
  }
  tab.thumbBtnEl.disabled = true;
  setThumbStatus(tab, 'pending', '正在生成缩略图(需解码视频帧,稍候)…');
  try {
    const result = await invoke('make_thumbnail', {
      input: tab.path,
      count,
      outputDir: tab.thumbDirEl.value.trim() || null,
    });
    setThumbStatus(tab, 'success', `缩略图已生成:${result.outputPath}`);
  } catch (err) {
    setThumbStatus(tab, 'error', String(err));
  } finally {
    tab.thumbBtnEl.disabled = false;
  }
}

function setThumbStatus(tab, kind, text) {
  // 保留 thumb-status 类,同时挂 crop-status 以便复用成功/失败/等待的颜色
  tab.thumbStatusEl.className = `thumb-status crop-status ${kind}`;
  tab.thumbStatusEl.textContent = text;
}

/* ---------- 打开文件(按钮 + 拖拽) ---------- */

openBtn.addEventListener('click', async () => {
  const invoke = getInvoke();
  if (!invoke) {
    alert('Tauri API 未注入(window.__TAURI__.core.invoke 不存在),无法打开文件选择器。请把这句话反馈给开发者。');
    return;
  }
  try {
    // 系统文件选择器,支持多选(Rust 端弹出)
    const paths = await invoke('open_files');
    if (Array.isArray(paths) && paths.length > 0) {
      paths.forEach((p) => openFile(p));
    }
    // 返回空数组 = 用户取消了选择,无需提示
  } catch (err) {
    alert('打开文件失败:' + err);
  }
});

/* 原生拖拽:一次拖入多个文件,逐个自动打开为标签页 */
try {
  const webview = getWebview();
  if (webview) {
    webview.onDragDropEvent((event) => {
      const p = event.payload;
      if (p.type === 'drop' && Array.isArray(p.paths)) {
        p.paths.forEach((path) => openFile(path));
      }
      const dragging = p.type === 'enter' || p.type === 'over';
      document.body.classList.toggle('dragging', dragging);
    });
  } else {
    showFatalBanner('window.__TAURI__.webview 不可用,拖拽功能失效');
  }
} catch (err) {
  showFatalBanner('注册拖拽监听失败:' + err);
}

/* 启动自检:页面加载成功 + Tauri API 状态 */
if (!window.__TAURI__) {
  showFatalBanner('window.__TAURI__ 不存在(脚本已加载,但 Tauri 全局 API 未注入)');
}
