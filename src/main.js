/* 媒体工具 - 前端逻辑(原生 JS,无框架)
 * 依赖 Tauri 注入的全局 API:window.__TAURI__(见 tauri.conf.json 的 withGlobalTauri)
 *   - core.invoke         调用 Rust 命令
 *   - webview ...DragDrop 监听原生文件拖拽事件
 */

const tauri = window.__TAURI__ || null;
const invoke = tauri ? tauri.core.invoke : null;

const tabsEl = document.getElementById('tabs');
const contentEl = document.getElementById('content');
const emptyEl = document.getElementById('empty-state');
const openBtn = document.getElementById('btn-open');

/** @type {Array<{id:number, path:string, name:string, tabEl:HTMLElement, pageEl:HTMLElement, infoEl:HTMLElement, statusEl:HTMLElement, btnEl:HTMLButtonElement, startEl:HTMLInputElement, endEl:HTMLInputElement, nameEl:HTMLInputElement}>} */
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

/**
 * 容错时间解析:支持 时:分:秒(1:23:45)、分:秒(5:30)、纯秒数(90 或 90.5)
 * @returns {number} 秒数;空串返回 null;格式非法返回 NaN
 */
function parseTime(str) {
  if (str == null) return null;
  str = String(str).trim();
  if (str === '') return null;               // 留空:仅对起始时间合法
  if (/^\d+(\.\d+)?$/.test(str)) return parseFloat(str);
  const m = str.match(/^(\d{1,4}):(\d{1,2})(?::(\d{1,2}(?:\.\d+)?))?$/);
  if (!m) return NaN;
  const a = parseInt(m[1], 10);
  const b = parseInt(m[2], 10);
  if (m[3] === undefined) return a * 60 + b;          // 两段:分:秒
  return a * 3600 + b * 60 + parseFloat(m[3]);        // 三段:时:分:秒
}

/* ---------- 标签页管理 ---------- */

/** 打开(或聚焦已打开的)文件:每个文件一个标签页,互相独立 */
async function openFile(path) {
  const existing = tabs.find((t) => t.path === path);
  if (existing) { activateTab(existing.id); return; }

  const tab = { id: ++tabSeq, path, name: fileNameOf(path) };
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

  // 标签页主体:上半文件信息、下半裁剪区
  tab.pageEl = document.createElement('div');
  tab.pageEl.className = 'tab-page';
  tab.pageEl.innerHTML = `
    <div class="filepath" title="${escapeHtml(path)}">文件:${escapeHtml(path)}</div>
    <div class="info-area"><div class="info-loading">正在读取媒体信息…</div></div>
    <div class="crop-area">
      <h2>快速裁剪(流复制,不重编码)</h2>
      <div class="crop-hint">时间格式支持 时:分:秒(如 1:23:45)、分:秒(如 5:30)或纯秒数(如 90);起始留空表示从开头开始</div>
      <div class="form-row">
        <label for="start-${tab.id}">起始时间</label>
        <input type="text" id="start-${tab.id}" class="crop-start" placeholder="00:00:00(可留空)">
        <span class="inline-hint">默认从文件开头</span>
      </div>
      <div class="form-row">
        <label for="end-${tab.id}">终止时间</label>
        <input type="text" id="end-${tab.id}" class="crop-end" placeholder="必填,如 1:23:45 或 90">
      </div>
      <div class="form-row">
        <label for="name-${tab.id}">输出文件名</label>
        <input type="text" id="name-${tab.id}" class="crop-name" placeholder="不含扩展名,如:我的片段">
        <span class="inline-hint">扩展名自动沿用源文件,保存到源文件所在目录</span>
      </div>
      <div class="form-actions">
        <button type="button" class="crop-btn">裁剪</button>
        <span class="crop-status"></span>
      </div>
    </div>`;
  contentEl.appendChild(tab.pageEl);

  tab.infoEl = tab.pageEl.querySelector('.info-area');
  tab.statusEl = tab.pageEl.querySelector('.crop-status');
  tab.btnEl = tab.pageEl.querySelector('.crop-btn');
  tab.startEl = tab.pageEl.querySelector('.crop-start');
  tab.endEl = tab.pageEl.querySelector('.crop-end');
  tab.nameEl = tab.pageEl.querySelector('.crop-name');

  tab.btnEl.addEventListener('click', () => runCrop(tab));

  activateTab(tab.id);

  // 异步探测媒体信息,失败时在信息区显示错误(不影响裁剪区展示)
  try {
    const sections = await invoke('probe_media', { path });
    if (!tabs.includes(tab)) return; // 探测期间标签可能已被关闭
    renderInfo(tab, sections);
  } catch (err) {
    if (!tabs.includes(tab)) return;
    tab.infoEl.innerHTML = `<div class="info-error">${escapeHtml(String(err))}</div>`;
  }
}

/** 切换到指定标签页 */
function activateTab(id) {
  tabs.forEach((t) => {
    const active = t.id === id;
    t.tabEl.classList.toggle('active', active);
    t.pageEl.classList.toggle('active', active);
  });
}

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
        ${sec.items.map((it) =>
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

  if (start === null && tab.startEl.value.trim() !== '') {
    setStatus(tab, 'error', '起始时间格式不对:支持 时:分:秒(1:23:45)、分:秒(5:30)或纯秒数(90)');
    return;
  }
  if (isNaN(start) || start < 0) {
    setStatus(tab, 'error', '起始时间格式不对:支持 时:分:秒(1:23:45)、分:秒(5:30)或纯秒数(90)');
    return;
  }
  if (end === null || isNaN(end)) {
    setStatus(tab, 'error', '请填写终止时间,格式如 1:23:45、5:30 或 90');
    return;
  }
  if (end <= 0) {
    setStatus(tab, 'error', '终止时间必须大于 0');
    return;
  }
  if (start !== null && start >= end) {
    setStatus(tab, 'error', '起始时间必须早于终止时间');
    return;
  }

  // 2. 输出文件名(不含扩展名,扩展名由 Rust 端沿用源文件)
  const outName = tab.nameEl.value.trim();
  if (!outName) {
    setStatus(tab, 'error', '请填写输出文件名(不含扩展名)');
    return;
  }
  if (/[\\/:]/.test(outName)) {
    setStatus(tab, 'error', '输出文件名不能包含 / \\ : 等字符');
    return;
  }

  // 3. 调用 Rust 命令执行裁剪(阻塞在后端线程,界面不卡)
  tab.btnEl.disabled = true;
  setStatus(tab, 'pending', '正在裁剪(流复制,通常几秒完成)…');
  try {
    const result = await invoke('crop_media', {
      input: tab.path,
      start: start,           // null 表示从开头
      end: end,
      outputName: outName,
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

/* ---------- 打开文件(按钮 + 拖拽) ---------- */

openBtn.addEventListener('click', async () => {
  if (!invoke) return;
  try {
    // 系统文件选择器,支持多选(Rust 端弹出)
    const paths = await invoke('open_files');
    (paths || []).forEach(openFile);
  } catch (err) {
    alert(`打开文件失败:${err}`);
  }
});

if (tauri && tauri.webview && tauri.webview.getCurrentWebview) {
  // 原生拖拽:一次拖入多个文件,逐个自动打开为标签页
  tauri.webview.getCurrentWebview().onDragDropEvent((event) => {
    const p = event.payload;
    if (p.type === 'drop' && Array.isArray(p.paths)) {
      p.paths.forEach((path) => openFile(path));
    }
    const dragging = p.type === 'enter' || p.type === 'over';
    document.body.classList.toggle('dragging', dragging);
  });
}
