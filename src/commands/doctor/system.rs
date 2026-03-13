#[path = "system/output.rs"]
mod output;
#[path = "system/probes.rs"]
mod probes;

use output::{
    finish_system_resources, print_apple_chip, print_gpu_lines, print_system_resources_header,
    print_total_ram,
};
use probes::{get_total_ram_gb, probe_apple_chip, probe_gpu_lines};

pub(super) fn check_system_resources() {
    print_system_resources_header();

    if let Some(total_ram_gb) = get_total_ram_gb() {
        print_total_ram(total_ram_gb);
    }

    if let Some(gpu_lines) = probe_gpu_lines() {
        print_gpu_lines(&gpu_lines);
    }

    if let Some(chip) = probe_apple_chip() {
        print_apple_chip(&chip);
    }

    finish_system_resources();
}
