// 桌面端入口。在 release 且 Windows 下隐藏控制台窗口。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    mediatool_lib::run()
}
