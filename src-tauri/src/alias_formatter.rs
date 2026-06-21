//! Reusable formatting + matching for model/feature aliases.

/// Feature tokens that must stay fully uppercase.
const ACRONYMS: &[&str] = &["MTP", "VLM", "GPU", "CPU", "CUDA", "ROCM", "API"];

/// Known model family names mapped to their pretty form.
fn pretty_family(token: &str) -> String {
    match token.to_lowercase().as_str() {
        "qwen" => "Qwen".to_string(),
        "llama" => "Llama".to_string(),
        "mistral" => "Mistral".to_string(),
        "gemma" => "Gemma".to_string(),
        "phi" => "Phi".to_string(),
        "deepseek" => "DeepSeek".to_string(),
        _ => capitalize(token),
    }
}

fn capitalize(token: &str) -> String {
    let mut chars = token.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Is the token a size marker like `27B`, `4b`, `7B`?
fn is_size_token(token: &str) -> bool {
    let bytes: Vec<char> = token.chars().collect();
    if bytes.len() < 2 {
        return false;
    }
    let (digits, rest) = bytes.split_at(bytes.iter().take_while(|c| c.is_ascii_digit()).count());
    !digits.is_empty()
        && !rest.is_empty()
        && rest.iter().all(|c| c.is_ascii_alphabetic())
}

/// Pretty-format a raw model name such as `qwen-27B` -> `Qwen-27B`.
pub fn pretty_model(raw: &str) -> String {
    let parts: Vec<String> = raw
        .split('-')
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .map(|token| {
            if is_size_token(token) {
                // Preserve the number, force the unit (B) uppercase: 27b -> 27B.
                token.to_uppercase()
            } else {
                pretty_family(token)
            }
        })
        .collect();
    parts.join("-")
}

/// Pretty-format a raw feature name. Acronyms stay uppercase, everything else is
/// title-cased unless it is already uppercase in the source.
pub fn pretty_feature(raw: &str) -> String {
    let trimmed = raw.trim();
    let upper = trimmed.to_uppercase();
    if ACRONYMS.contains(&upper.as_str()) {
        return upper;
    }
    // Already an uppercase acronym in the source (e.g. "VLM").
    let has_alpha = trimmed.chars().any(|c| c.is_alphabetic());
    let no_lower = !trimmed.chars().any(|c| c.is_lowercase());
    if has_alpha && no_lower {
        return trimmed.to_string();
    }
    title_case(trimmed)
}

fn title_case(s: &str) -> String {
    s.split(|c: char| c == ' ' || c == '-' || c == '_')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let upper = w.to_uppercase();
            if ACRONYMS.contains(&upper.as_str()) {
                upper
            } else {
                capitalize(&w.to_lowercase())
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Build the display alias: `{PrettyModel} {PrettyFeature}`.
pub fn build_alias(pretty_model: &str, pretty_feature: &str) -> String {
    format!("{} {}", pretty_model, pretty_feature)
}

/// Lowercase, sanitized internal id component (spaces/dashes collapsed).
pub fn sanitize_id_part(raw: &str) -> String {
    let mut out = String::new();
    let mut prev_sep = false;
    for c in raw.trim().to_lowercase().chars() {
        if c.is_alphanumeric() {
            out.push(c);
            prev_sep = false;
        } else if !prev_sep {
            out.push('-');
            prev_sep = true;
        }
    }
    out.trim_matches('-').to_string()
}

/// Build a stable internal id from raw model + feature: `qwen-27b__mtp`.
pub fn build_id(raw_model: &str, raw_feature: &str) -> String {
    format!(
        "{}__{}",
        sanitize_id_part(raw_model),
        sanitize_id_part(raw_feature)
    )
}

/// Normalize an alias for case-insensitive, separator-flexible matching.
/// "Qwen-27B MTP", "qwen 27b mtp" and "Qwen 27B MTP" all normalize equally.
pub fn normalize_alias(s: &str) -> String {
    let lowered = s.to_lowercase();
    let spaced: String = lowered
        .chars()
        .map(|c| if c == '-' || c == '_' { ' ' } else { c })
        .collect();
    spaced.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pretty_models() {
        assert_eq!(pretty_model("qwen-27B"), "Qwen-27B");
        assert_eq!(pretty_model("qwen-4B"), "Qwen-4B");
        assert_eq!(pretty_model("llama-70B"), "Llama-70B");
        assert_eq!(pretty_model("mistral-7B"), "Mistral-7B");
        // size casing preserved/normalized regardless of input case.
        assert_eq!(pretty_model("qwen-32b"), "Qwen-32B");
    }

    #[test]
    fn pretty_features() {
        assert_eq!(pretty_feature("MTP"), "MTP");
        assert_eq!(pretty_feature("VLM"), "VLM");
        assert_eq!(pretty_feature("CPU"), "CPU");
        assert_eq!(pretty_feature("Vision"), "Vision");
        assert_eq!(pretty_feature("vision"), "Vision");
        assert_eq!(pretty_feature("Standard"), "Standard");
        assert_eq!(pretty_feature("cuda"), "CUDA");
    }

    #[test]
    fn aliases_and_ids() {
        assert_eq!(
            build_alias(&pretty_model("qwen-27B"), &pretty_feature("MTP")),
            "Qwen-27B MTP"
        );
        assert_eq!(build_id("qwen-27B", "MTP"), "qwen-27b__mtp");
    }

    #[test]
    fn alias_matching_is_flexible() {
        let target = normalize_alias("Qwen-27B MTP");
        assert_eq!(normalize_alias("qwen-27b mtp"), target);
        assert_eq!(normalize_alias("Qwen 27B MTP"), target);
        assert_ne!(normalize_alias("Qwen-4B MTP"), target);
    }
}
