pub(super) fn print_system_resources_header() {
    println!("System Resources:");
}

pub(super) fn print_total_ram(total_ram_gb: f64) {
    println!("  Total RAM: {total_ram_gb:.1} GB");
}

pub(super) fn print_gpu_lines(gpu_lines: &[String]) {
    for line in gpu_lines {
        println!("  GPU: {line}");
    }
}

pub(super) fn print_apple_chip(chip: &str) {
    #[cfg(target_os = "macos")]
    {
        println!("  Chip: {chip} (unified memory, GPU acceleration available)");
    }

    #[cfg(not(target_os = "macos"))]
    let _ = chip;
}

pub(super) fn finish_system_resources() {
    println!();
}
