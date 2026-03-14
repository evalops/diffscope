use crate::core::offline::LocalModel;

pub(in super::super) fn print_endpoint_models(endpoint_type: &str, models: &[LocalModel]) {
    println!("\nEndpoint type: {endpoint_type}");
    println!("\nAvailable models ({}):", models.len());
    if models.is_empty() {
        println!("  (none found)");
        if endpoint_type == "ollama" {
            println!("\n  Pull a model: ollama pull codellama");
        }
        return;
    }

    for model in models {
        println!("  - {}{}", model.name, format_model_size_info(model));
    }
}

fn format_model_size_info(model: &LocalModel) -> String {
    if model.size_mb == 0 {
        return String::new();
    }

    format!(" ({}MB", model.size_mb)
        + &model
            .quantization
            .as_ref()
            .map(|quantization| format!(", {quantization}"))
            .unwrap_or_default()
        + ")"
}
