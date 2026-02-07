#[cfg(target_os = "linux")]
use std::path::Path;
#[cfg(target_os = "macos")]
use std::process::Command;

pub(crate) fn build_system_info() -> String {
    let os_info = os_info::get();
    let os = format!("{} {}", os_info.os_type(), os_info.version());
    let cores = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1);
    let ram = total_memory_gb()
        .map(|gb| format!("{gb:.1} GB"))
        .unwrap_or_else(|| "unknown".to_string());
    let gpu = detect_gpu().unwrap_or_else(|| "unknown".to_string());

    format!("# System Info\n- OS: {os}\n- CPU Cores: {cores}\n- RAM: {ram}\n- GPU: {gpu}\n",)
}

fn total_memory_gb() -> Option<f64> {
    let mem_kb = read_mem_total_kb()?;
    Some(mem_kb as f64 / 1024.0 / 1024.0)
}

#[cfg(target_os = "linux")]
fn read_mem_total_kb() -> Option<u64> {
    let contents = std::fs::read_to_string("/proc/meminfo").ok()?;
    for line in contents.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            let kb = rest.split_whitespace().next()?;
            return kb.parse::<u64>().ok();
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn read_mem_total_kb() -> Option<u64> {
    let bytes = command_output("sysctl", &["-n", "hw.memsize"])?;
    let bytes = bytes.trim().parse::<u64>().ok()?;
    Some(bytes / 1024)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn read_mem_total_kb() -> Option<u64> {
    None
}

fn detect_gpu() -> Option<String> {
    if let Some(gpu) = detect_gpu_macos() {
        return Some(gpu);
    }
    detect_nvidia_gpu()
}

#[cfg(target_os = "linux")]
fn detect_nvidia_gpu() -> Option<String> {
    let root = Path::new("/proc/driver/nvidia/gpus");
    let entries = std::fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let info_path = entry.path().join("information");
        let contents = std::fs::read_to_string(info_path).ok()?;
        for line in contents.lines() {
            if let Some(model) = line.strip_prefix("Model:") {
                let model = model.trim();
                if !model.is_empty() {
                    return Some(model.to_string());
                }
            }
        }
    }
    None
}

#[cfg(not(target_os = "linux"))]
fn detect_nvidia_gpu() -> Option<String> {
    None
}

#[cfg(target_os = "macos")]
fn detect_gpu_macos() -> Option<String> {
    let output = command_output(
        "system_profiler",
        &["SPDisplaysDataType", "-detailLevel", "mini"],
    )?;
    for line in output.lines() {
        let line = line.trim();
        if let Some(model) = line.strip_prefix("Chipset Model:") {
            let model = model.trim();
            if !model.is_empty() {
                return Some(model.to_string());
            }
        }
        if let Some(model) = line.strip_prefix("Model:") {
            let model = model.trim();
            if !model.is_empty() {
                return Some(model.to_string());
            }
        }
    }
    None
}

#[cfg(not(target_os = "macos"))]
fn detect_gpu_macos() -> Option<String> {
    None
}

#[cfg(target_os = "macos")]
fn command_output(command: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(command).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}
