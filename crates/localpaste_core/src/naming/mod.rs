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

        if let Some(name) =
            crate::semantic::extract_definition_handle_from_line(candidate, Some(lang.as_str()))
        {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_language_specific_titles() {
        let cases = [
            ("# Hello World\nbody", Some("markdown"), Some("Hello World")),
            (
                "fn handle_request(req: Request) -> Response {}",
                Some("rust"),
                Some("fn handle_request"),
            ),
            (
                "export function renderPanel() {}",
                Some("typescript"),
                Some("export function renderPanel"),
            ),
            (
                "export const renderPanel = () => {}",
                Some("typescript"),
                Some("export const renderPanel"),
            ),
            (
                "export class WorkspacePanel {}",
                Some("javascript"),
                Some("export class WorkspacePanel"),
            ),
        ];
        for (content, language, expected) in cases {
            let derived = derive_name_from_content(content, language);
            assert_eq!(derived.as_deref(), expected);
        }
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
