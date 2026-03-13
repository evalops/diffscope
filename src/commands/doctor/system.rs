pub(super) fn check_system_resources() {
    println!("System Resources:");

    if let Some(total_ram_gb) = get_total_ram_gb() {
        println!("  Total RAM: {:.1} GB", total_ram_gb);
    }

    if let Ok(output) = std::process::Command::new("nvidia-smi")
        .arg("--query-gpu=name,memory.total,memory.free")
        .arg("--format=csv,noheader,nounits")
        .output()
    {
        if output.status.success() {
            let gpu_info = String::from_utf8_lossy(&output.stdout);
            for line in gpu_info.trim().lines() {
                println!("  GPU: {}", line.trim());
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = std::process::Command::new("sysctl")
            .arg("-n")
            .arg("machdep.cpu.brand_string")
            .output()
        {
            if output.status.success() {
                let cpu = String::from_utf8_lossy(&output.stdout);
                let cpu = cpu.trim();
                if cpu.contains("Apple") {
                    println!(
                        "  Chip: {} (unified memory, GPU acceleration available)",
                        cpu
                    );
                }
            }
        }
    }

    println!();
}

fn get_total_ram_gb() -> Option<f64> {
    #[cfg(target_os = "macos")]
    {
        get_total_ram_gb_macos()
    }
    #[cfg(target_os = "linux")]
    {
        get_total_ram_gb_linux()
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

#[cfg(target_os = "macos")]
fn get_total_ram_gb_macos() -> Option<f64> {
    let output = std::process::Command::new("sysctl")
        .arg("-n")
        .arg("hw.memsize")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let bytes: u64 = text.trim().parse().ok()?;
    Some(bytes as f64 / (1024.0 * 1024.0 * 1024.0))
}

#[cfg(target_os = "linux")]
fn get_total_ram_gb_linux() -> Option<f64> {
    let content = std::fs::read_to_string("/proc/meminfo").ok()?;
    for line in content.lines() {
        if line.starts_with("MemTotal:") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let kb: u64 = parts[1].parse().ok()?;
                return Some(kb as f64 / (1024.0 * 1024.0));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_total_ram_gb() {
        let ram = get_total_ram_gb();

        #[cfg(any(target_os = "macos", target_os = "linux"))]
        assert!(ram.is_some(), "Should detect RAM on macOS/Linux");
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            let gb = ram.unwrap();
            assert!(gb > 0.5, "RAM should be at least 0.5 GB, got {}", gb);
            assert!(gb < 4096.0, "RAM should be under 4 TB, got {}", gb);
        }

        let _ = ram;
    }
}
