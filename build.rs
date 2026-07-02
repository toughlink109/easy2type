//! build.rs — 构建脚本
//!
//! 职责（按顺序执行）：
//! 1. 从 GitHub 自动下载 Google Trillion Word Corpus 高频词库（10 万词完整版）
//! 2. 清洗源数据：去除空行、统一小写、分配词频权重
//! 3. 输出为 `assets/words.txt`（word<TAB>frequency 格式）
//!
//! # 跳过逻辑
//! 如果 `assets/words.txt` 已存在且行数 ≥ 1000，则跳过下载。
//! 如需强制重新下载，删除 `assets/words.txt` 后重新编译即可。
//!
//! # 权重分配规则
//! 源文件按词频降序排列（第一行最高频），我们将行号倒序映射为权重：
//!   第 1 行 → 权重 = 总行数
//!   第 N 行 → 权重 = 总行数 - N + 1
//! 即最高频词获得最大权重值。

use std::fs;
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::process::Command;

/// 10 万词完整版 — 基于 Google Trillion Word Corpus
const WORD_LIST_URL: &str =
    "https://raw.githubusercontent.com/first20hours/google-10000-english/master/google-10000-english-no-swears.txt";

/// 1 万词精简版（备选，适合快速测试）
const _WORD_LIST_URL_LITE: &str =
    "https://raw.githubusercontent.com/first20hours/google-10000-english/master/google-10000-english-usa-no-swears-medium.txt";

/// 最终输出文件
const OUTPUT_PATH: &str = "assets/words.txt";

/// 临时原始下载文件
const TEMP_PATH: &str = "assets/words_raw.txt";

/// 跳过下载的最小行数阈值
const SKIP_MIN_LINES: usize = 1000;

fn main() {
    // ═══ 0. 编译 Slint UI 文件（仅在 slint-ui feature 启用时） ═══
    #[cfg(feature = "slint-ui")]
    slint_build::compile("ui/settings.slint").expect("Slint UI 编译失败");

    // ═══ 0b. 生成/嵌入图标资源 ═══
    generate_icon_if_missing();
    embed_icon();

    // ═══ 1. 判断是否需要下载 ═══
    if Path::new(OUTPUT_PATH).exists() {
        let existing = count_lines(OUTPUT_PATH);
        if existing >= SKIP_MIN_LINES {
            println!(
                "cargo:warning=[Build] assets/words.txt 已存在 ({} 行)，跳过下载",
                existing
            );
            return;
        }
        println!(
            "cargo:warning=[Build] 现有词库仅 {} 行（阈值 {}），触发重新下载",
            existing, SKIP_MIN_LINES
        );
    }

    // ═══ 2. 下载 ═══
    println!("cargo:warning=[Build] 正在下载词库...");
    download_word_list();

    // ═══ 3. 清洗与格式化 ═══
    println!("cargo:warning=[Build] 正在清洗词库...");
    let count = clean_and_format();

    // ═══ 4. 清理临时文件 ═══
    let _ = fs::remove_file(TEMP_PATH);

    println!(
        "cargo:warning=[Build] ✓ 词库清洗完成: {} 个单词 → {}",
        count, OUTPUT_PATH
    );
}

/// 调用系统命令下载原始词库文件
fn download_word_list() {
    // 确保 assets 目录存在
    fs::create_dir_all("assets").expect("创建 assets 目录失败");

    // 优先使用 curl（Windows 10+ 内置）
    let curl_result = Command::new("curl")
        .args([
            "-L",                      // 跟随重定向
            "-o", TEMP_PATH,           // 输出到临时文件
            "--connect-timeout", "30", // 连接超时
            "--max-time", "120",       // 总超时
            "-s",                      // 静默（无进度条）
            "-S",                      // 但显示错误
            WORD_LIST_URL,
        ])
        .output();

    match curl_result {
        Ok(output) if output.status.success() => {
            println!("cargo:warning=[Build] 词库下载完成 (curl)");
            return;
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            println!(
                "cargo:warning=[Build] curl 下载失败 (code {:?})，回退到 PowerShell...",
                output.status.code()
            );
            if !stderr.trim().is_empty() {
                println!("cargo:warning=[Build] curl stderr: {}", stderr.trim());
            }
        }
        Err(e) => {
            println!(
                "cargo:warning=[Build] curl 不可用 ({})，回退到 PowerShell...",
                e
            );
        }
    }

    // 回退：PowerShell Invoke-WebRequest
    let ps_output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &format!(
                "try {{ Invoke-WebRequest -Uri '{}' -OutFile '{}' -TimeoutSec 120 }} catch {{ Write-Error $_.Exception.Message; exit 1 }}",
                WORD_LIST_URL, TEMP_PATH
            ),
        ])
        .output()
        .expect("下载词库失败：无法启动 PowerShell");

    if !ps_output.status.success() {
        let stderr = String::from_utf8_lossy(&ps_output.stderr);
        panic!(
            "下载词库失败（curl + PowerShell 均未成功）:\n\
             ========== stderr ==========\n\
             {}\n\
             ============================\n\
             请检查网络连接后重试，或手动下载词库文件放置到 {}。",
            stderr.trim(), OUTPUT_PATH
        );
    }

    println!("cargo:warning=[Build] 词库下载完成 (PowerShell)");
}

/// 清洗原始词库并输出格式化文件
///
/// 源格式：每行一个单词，按词频降序排列
/// 输出格式：word<tab>frequency
fn clean_and_format() -> usize {
    let file = fs::File::open(TEMP_PATH).expect("无法打开下载的原始词库文件");

    let reader = io::BufReader::new(file);

    // 收集所有有效单词
    let words: Vec<String> = reader
        .lines()
        .filter_map(|line| {
            let line = line.ok()?;
            let word = line.trim().to_lowercase();
            // 过滤空行和含空白字符的行
            if word.is_empty() || word.contains(char::is_whitespace) {
                None
            } else {
                Some(word)
            }
        })
        .collect();

    let total = words.len();

    if total == 0 {
        panic!("清洗后的词库为空，请检查下载的原始文件是否有效");
    }

    // 写入格式化文件：word<tab>frequency
    // 权重 = total - index（第一行权重最高）
    let mut output = fs::File::create(OUTPUT_PATH).expect("无法创建输出文件");

    for (i, word) in words.iter().enumerate() {
        let weight = total - i;
        writeln!(output, "{}\t{}", word, weight).expect("写入词库文件失败");
    }

    // 确保落盘
    output.flush().expect("刷新词库文件失败");

    total
}

/// 统计文件行数
fn count_lines(path: &str) -> usize {
    fs::read_to_string(path)
        .map(|s| s.lines().count())
        .unwrap_or(0)
}

/// 图标资源 ID（与 config.rs 中 IDI_ICON_ID 一致）
const ICON_RESOURCE_ID: u16 = 1; // winres 默认主图标 ID

/// 如果 assets/icon.ico 不存在，自动生成一个 32x32 蓝底 "e" 图标
fn generate_icon_if_missing() {
    if Path::new("assets/icon.ico").exists() {
        return;
    }
    println!("cargo:warning=[Build] 正在生成 assets/icon.ico...");
    fs::create_dir_all("assets").ok();
    let ico = build_ico_bytes();
    if fs::write("assets/icon.ico", &ico).is_ok() {
        println!("cargo:warning=[Build] assets/icon.ico 生成成功 ({} bytes)", ico.len());
    } else {
        println!("cargo:warning=[Build] assets/icon.ico 写入失败");
    }
}

/// 构建 32x32 BGRA .ico 文件字节（蓝底 #4A90E2 + 白色几何 "e" 字）
fn build_ico_bytes() -> Vec<u8> {
    let mut buf = Vec::with_capacity(4286);

    // ── ICO Header (6 bytes) ──
    buf.extend_from_slice(&[0, 0]);          // reserved
    buf.extend_from_slice(&[1, 0]);          // type = ICO
    buf.extend_from_slice(&[1, 0]);          // count = 1

    // ── Directory Entry (16 bytes) ──
    let data_offset: u32 = 6 + 16;           // header + 1 entry
    let image_size: u32 = 40 + 32*32*4 + 128; // infoheader + pixels + mask
    buf.push(32);  buf.push(32);             // width 32, height 32
    buf.push(0);   buf.push(0);              // palette, reserved
    buf.extend_from_slice(&[1, 0]);          // planes
    buf.extend_from_slice(&[32, 0]);         // bpp = 32
    buf.extend_from_slice(&image_size.to_le_bytes());
    buf.extend_from_slice(&data_offset.to_le_bytes());

    // ── BITMAPINFOHEADER (40 bytes) ──
    buf.extend_from_slice(&40u32.to_le_bytes());  // biSize
    buf.extend_from_slice(&32i32.to_le_bytes());  // biWidth
    buf.extend_from_slice(&64i32.to_le_bytes());  // biHeight (2×32 for ICO)
    buf.extend_from_slice(&[1, 0]);               // biPlanes
    buf.extend_from_slice(&[32, 0]);              // biBitCount
    buf.extend_from_slice(&0u32.to_le_bytes());   // biCompression
    buf.extend_from_slice(&0u32.to_le_bytes());   // biSizeImage
    buf.extend_from_slice(&0i32.to_le_bytes());   // biXPelsPerMeter
    buf.extend_from_slice(&0i32.to_le_bytes());   // biYPelsPerMeter
    buf.extend_from_slice(&0u32.to_le_bytes());   // biClrUsed
    buf.extend_from_slice(&0u32.to_le_bytes());   // biClrImportant

    // ── 32×32 BGRA 像素（几何 "e" 字：蓝底 + 白色环形 + 蓝缺口） ──
    let cx: f32 = 16.0;
    let cy: f32 = 16.0;
    for y in 0..32 {
        for x in 0..32 {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let dist = (dx * dx + dy * dy).sqrt();

            // 白色圆环（外径 11, 内径 7 的左大半部分 → 形成 "e" 形状）
            let outer = dist <= 11.0;
            let inner = dist <= 7.0;
            // 右侧缺口：缺口中心在 (20, 16)
            let gx = x as f32 - 20.0;
            let gy = y as f32 - 16.0;
            let gap = (gx * gx + gy * gy).sqrt() <= 5.5;
            // 上半缺口：缺口中心在 (16, 10)
            let hx = x as f32 - 16.0;
            let hy = y as f32 - 24.0;
            let top_gap = (hx * hx + hy * hy).sqrt() <= 5.0;

            let is_white = outer && !inner && !gap && !top_gap;
            // 中心填充
            let center_fill = dist <= 7.0 && !gap && !top_gap;

            let (r, g, b) = if is_white || center_fill {
                (0xFFu8, 0xFFu8, 0xFFu8)
            } else {
                (0xE2u8, 0x90u8, 0x4Au8) // #4A90E2 → BGRA
            };

            buf.push(b); buf.push(g); buf.push(r); buf.push(0xFF); // BGRA
        }
    }

    // ── AND Mask (128 bytes, 全不透明) ──
    buf.extend_from_slice(&[0xFFu8; 128]);

    buf
}

/// 使用 winres 嵌入图标资源（仅 MSVC 工具链可用）
///
/// 将 assets/icon.ico 绑定到资源 ID 101。
/// GNU 工具链时静默跳过 —— 运行时回退到 GDI 程序化图标。
fn embed_icon() {
    if !Path::new("assets/icon.ico").exists() {
        println!("cargo:warning=[Build] assets/icon.ico 不存在，跳过图标嵌入");
        return;
    }

    match std::panic::catch_unwind(|| {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("InternalName", "easy2type");
        res.set("ProductName", "easy2type");
        res.set("OriginalFilename", "easy2type.exe");
        if let Err(e) = res.compile() {
            println!("cargo:warning=[Build] winres 编译失败 (非 MSVC?): {}", e);
        } else {
            println!("cargo:warning=[Build] 图标资源嵌入成功 (ID={})", ICON_RESOURCE_ID);
        }
    }) {
        Ok(_) => {}
        Err(_) => {
            println!("cargo:warning=[Build] winres 不可用 (GNU 工具链?), 跳过图标嵌入");
        }
    }
}
