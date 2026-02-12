//! Utilities for generating human-friendly paste names.

use rand::Rng;

const ADJECTIVES: &[&str] = &[
    "ethereal",
    "quantum",
    "cosmic",
    "stellar",
    "nebula",
    "aurora",
    "crystal",
    "mystic",
    "velvet",
    "golden",
    "silver",
    "shadow",
    "lunar",
    "solar",
    "arctic",
    "tropical",
    "ancient",
    "eternal",
    "infinite",
    "serene",
    "vibrant",
    "radiant",
    "electric",
    "magnetic",
    "atomic",
    "dynamic",
    "harmonic",
    "melodic",
    "rhythmic",
    "prismatic",
    "holographic",
    "virtual",
    "digital",
    "analog",
    "binary",
    "hexagon",
    "spiral",
    "fractal",
    "geometric",
    "abstract",
    "minimal",
    "epic",
    "legendary",
    "mythic",
    "heroic",
    "noble",
    "royal",
    "imperial",
    "zen",
    "tranquil",
    "peaceful",
    "wild",
    "untamed",
    "fierce",
    "bold",
    "brave",
    "swift",
    "rapid",
    "turbo",
    "hyper",
    "mega",
    "ultra",
    "super",
    "prime",
    "alpha",
    "beta",
    "gamma",
    "delta",
    "omega",
    "sigma",
    "lambda",
    "phoenix",
];

const NOUNS: &[&str] = &[
    "taco",
    "pizza",
    "burger",
    "sushi",
    "ramen",
    "pasta",
    "cookie",
    "donut",
    "dragon",
    "phoenix",
    "unicorn",
    "griffin",
    "sphinx",
    "kraken",
    "hydra",
    "pegasus",
    "ninja",
    "samurai",
    "viking",
    "pirate",
    "knight",
    "wizard",
    "sage",
    "oracle",
    "comet",
    "meteor",
    "galaxy",
    "nebula",
    "quasar",
    "pulsar",
    "cosmos",
    "universe",
    "wave",
    "tide",
    "ocean",
    "river",
    "stream",
    "cascade",
    "waterfall",
    "geyser",
    "mountain",
    "valley",
    "canyon",
    "plateau",
    "summit",
    "peak",
    "ridge",
    "cliff",
    "forest",
    "jungle",
    "desert",
    "tundra",
    "savanna",
    "prairie",
    "meadow",
    "grove",
    "crystal",
    "diamond",
    "ruby",
    "emerald",
    "sapphire",
    "opal",
    "pearl",
    "jade",
    "thunder",
    "lightning",
    "storm",
    "tempest",
    "blizzard",
    "hurricane",
    "tornado",
    "cyclone",
    "code",
    "cipher",
    "matrix",
    "nexus",
    "portal",
    "gateway",
    "bridge",
    "beacon",
    "echo",
    "whisper",
    "shadow",
    "phantom",
    "specter",
    "spirit",
    "ghost",
    "wraith",
    "flame",
    "spark",
    "ember",
    "inferno",
    "blaze",
    "fire",
    "torch",
    "flare",
];

/// Generate a random adjective-noun name.
///
/// # Returns
/// A randomly composed name.
///
/// # Panics
/// Does not intentionally panic.
pub fn generate_name() -> String {
    let mut rng = rand::thread_rng();
    let adj = ADJECTIVES[rng.gen_range(0..ADJECTIVES.len())];
    let noun = NOUNS[rng.gen_range(0..NOUNS.len())];
    format!("{}-{}", adj, noun)
}

/// Generate a unique name, with collision handling.
///
/// Tries base name first, then appends a random suffix if needed.
///
/// # Returns
/// A name that does not collide according to `exists_check`.
pub fn generate_unique_name<F>(exists_check: F) -> String
where
    F: Fn(&str) -> bool,
{
    // Try up to 5 times with just adjective-noun
    for _ in 0..5 {
        let name = generate_name();
        if !exists_check(&name) {
            return name;
        }
    }

    // If still colliding, append a random suffix
    let mut rng = rand::thread_rng();
    loop {
        let base = generate_name();
        let suffix: u32 = rng.gen_range(1000..9999);
        let name = format!("{}-{}", base, suffix);
        if !exists_check(&name) {
            return name;
        }
    }
}

/// Derive a human-readable paste name from content.
///
/// Returns `None` when content is empty or no meaningful line can be extracted.
///
/// # Arguments
/// - `content`: Paste text used to infer a title.
/// - `language`: Optional detected/manual language hint.
///
/// # Returns
/// A derived title when a meaningful line can be extracted, otherwise `None`.
pub fn derive_name_from_content(content: &str, language: Option<&str>) -> Option<String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }

    let lang = language.unwrap_or_default().to_ascii_lowercase();
    if lang == "markdown" {
        for line in content.lines() {
            let candidate = line.trim().trim_start_matches('#').trim();
            if line.trim_start().starts_with('#') && !candidate.is_empty() {
                return Some(truncate_name(candidate, 48));
            }
        }
    }

    for line in content.lines() {
        let candidate = line.trim();
        if candidate.is_empty()
            || candidate.starts_with("//")
            || candidate.starts_with('#')
            || candidate.starts_with("/*")
        {
            continue;
        }

        if let Some(name) = extract_definition_name(candidate, &lang) {
            return Some(truncate_name(name.as_str(), 48));
        }

        return Some(truncate_name(candidate, 48));
    }

    None
}

/// Prefer a content-derived name and fall back to random adjective-noun.
///
/// # Arguments
/// - `content`: Paste text used to infer a title.
/// - `language`: Optional detected/manual language hint.
///
/// # Returns
/// A content-derived title when possible; otherwise a random generated name.
pub fn generate_name_for_content(content: &str, language: Option<&str>) -> String {
    derive_name_from_content(content, language).unwrap_or_else(generate_name)
}

fn truncate_name(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect::<String>()
}

fn extract_definition_name(line: &str, language: &str) -> Option<String> {
    let patterns: &[&str] = match language {
        "rust" => &["fn ", "struct ", "enum ", "trait ", "impl "],
        "python" => &["def ", "class ", "async def "],
        "javascript" | "typescript" => &["function ", "class ", "const ", "export "],
        "go" => &["func ", "type ", "package "],
        _ => return None,
    };

    for pattern in patterns {
        if let Some(rest) = line.strip_prefix(pattern) {
            let ident: String = rest
                .chars()
                .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
                .collect();
            if !ident.is_empty() {
                return Some(format!("{} {}", pattern.trim(), ident));
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_markdown_heading() {
        let content = "# Hello World\nbody";
        let derived = derive_name_from_content(content, Some("markdown"));
        assert_eq!(derived.as_deref(), Some("Hello World"));
    }

    #[test]
    fn derives_rust_function_name() {
        let content = "fn handle_request(req: Request) -> Response {}";
        let derived = derive_name_from_content(content, Some("rust"));
        assert_eq!(derived.as_deref(), Some("fn handle_request"));
    }

    #[test]
    fn skips_comment_lines_and_uses_first_meaningful_line() {
        let content = "// comment\n# metadata\nactual line";
        let derived = derive_name_from_content(content, None);
        assert_eq!(derived.as_deref(), Some("actual line"));
    }

    #[test]
    fn content_name_falls_back_to_random() {
        let generated = generate_name_for_content("", None);
        assert!(!generated.is_empty());
    }
}
