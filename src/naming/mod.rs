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
#[allow(dead_code)]
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
