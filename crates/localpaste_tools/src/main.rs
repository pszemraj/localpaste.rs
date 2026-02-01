//! Test data generator for LocalPaste performance testing.
//!
//! Generates synthetic pastes with varied sizes and languages
//! for benchmarking and stress testing the GUI.
//!
//! # Usage
//!
//! ```bash
//! # Generate 1000 pastes (default)
//! cargo run -p localpaste_tools --bin generate-test-data
//!
//! # Generate 10k pastes with 50 folders
//! cargo run -p localpaste_tools --bin generate-test-data -- --count 10000 --folders 50
//!
//! # Clear existing data first
//! cargo run -p localpaste_tools --bin generate-test-data -- --clear --count 5000
//!
//! # Use custom database path
//! DB_PATH=/tmp/test-db cargo run -p localpaste_tools --bin generate-test-data -- --count 100
//! ```

use clap::Parser;
use localpaste_core::{config::Config, db::Database, models::folder::Folder, models::paste::Paste};
use rand::prelude::*;
use std::time::Instant;

/// Test data generator for LocalPaste performance testing.
#[derive(Parser)]
#[command(
    name = "generate-test-data",
    about = "Generate synthetic test data for LocalPaste"
)]
struct Args {
    /// Number of pastes to generate
    #[arg(short, long, default_value = "1000")]
    count: usize,

    /// Number of folders to create
    #[arg(short, long, default_value = "20")]
    folders: usize,

    /// Clear existing data before generating
    #[arg(long)]
    clear: bool,

    /// Print progress every N pastes
    #[arg(long, default_value = "100")]
    progress_interval: usize,
}

/// Language templates with realistic code snippets.
struct LanguageTemplate {
    id: &'static str,
    samples: &'static [&'static str],
}

const LANGUAGES: &[LanguageTemplate] = &[
    LanguageTemplate {
        id: "rust",
        samples: &[
            r#"fn main() {
    println!("Hello, world!");
}
"#,
            r#"use std::collections::HashMap;

fn process_data(items: &[i32]) -> HashMap<i32, usize> {
    let mut counts = HashMap::new();
    for &item in items {
        *counts.entry(item).or_insert(0) += 1;
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_data() {
        let data = vec![1, 2, 2, 3, 3, 3];
        let result = process_data(&data);
        assert_eq!(result.get(&1), Some(&1));
        assert_eq!(result.get(&2), Some(&2));
        assert_eq!(result.get(&3), Some(&3));
    }
}
"#,
            r#"#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub database_url: String,
    #[serde(default)]
    pub debug: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 8080,
            database_url: "sqlite://data.db".to_string(),
            debug: false,
        }
    }
}
"#,
        ],
    },
    LanguageTemplate {
        id: "python",
        samples: &[
            r#"def hello_world():
    print("Hello, world!")

if __name__ == "__main__":
    hello_world()
"#,
            r#"import json
from dataclasses import dataclass
from typing import List, Optional

@dataclass
class User:
    id: int
    name: str
    email: str
    active: bool = True

def load_users(filename: str) -> List[User]:
    with open(filename) as f:
        data = json.load(f)
    return [User(**item) for item in data]

def filter_active(users: List[User]) -> List[User]:
    return [u for u in users if u.active]
"#,
            r#"class DataProcessor:
    def __init__(self, config: dict):
        self.config = config
        self._cache = {}

    def process(self, data: list) -> dict:
        results = {}
        for item in data:
            key = self._extract_key(item)
            if key in self._cache:
                results[key] = self._cache[key]
            else:
                value = self._compute(item)
                self._cache[key] = value
                results[key] = value
        return results

    def _extract_key(self, item):
        return hash(tuple(sorted(item.items())))

    def _compute(self, item):
        return sum(v for v in item.values() if isinstance(v, (int, float)))
"#,
        ],
    },
    LanguageTemplate {
        id: "javascript",
        samples: &[
            r#"console.log("Hello, world!");
"#,
            r#"const express = require('express');
const app = express();

app.use(express.json());

app.get('/api/items', (req, res) => {
    res.json({ items: [], total: 0 });
});

app.post('/api/items', (req, res) => {
    const { name, value } = req.body;
    // Process and save item
    res.status(201).json({ id: Date.now(), name, value });
});

app.listen(3000, () => {
    console.log('Server running on port 3000');
});
"#,
            r#"class EventEmitter {
    constructor() {
        this.events = new Map();
    }

    on(event, callback) {
        if (!this.events.has(event)) {
            this.events.set(event, []);
        }
        this.events.get(event).push(callback);
        return this;
    }

    emit(event, ...args) {
        const callbacks = this.events.get(event) || [];
        callbacks.forEach(cb => cb(...args));
        return this;
    }

    off(event, callback) {
        const callbacks = this.events.get(event) || [];
        const index = callbacks.indexOf(callback);
        if (index !== -1) {
            callbacks.splice(index, 1);
        }
        return this;
    }
}

module.exports = EventEmitter;
"#,
        ],
    },
    LanguageTemplate {
        id: "json",
        samples: &[
            r#"{
    "name": "example",
    "version": "1.0.0"
}
"#,
            r#"{
    "users": [
        {"id": 1, "name": "Alice", "email": "alice@example.com"},
        {"id": 2, "name": "Bob", "email": "bob@example.com"},
        {"id": 3, "name": "Charlie", "email": "charlie@example.com"}
    ],
    "metadata": {
        "total": 3,
        "page": 1,
        "per_page": 10
    }
}
"#,
            r#"{
    "database": {
        "host": "localhost",
        "port": 5432,
        "name": "myapp",
        "credentials": {
            "username": "admin",
            "password_env": "DB_PASSWORD"
        }
    },
    "cache": {
        "enabled": true,
        "ttl_seconds": 3600,
        "max_entries": 10000
    },
    "features": {
        "dark_mode": true,
        "notifications": false,
        "experimental": ["feature_a", "feature_b"]
    }
}
"#,
        ],
    },
    LanguageTemplate {
        id: "markdown",
        samples: &[
            r#"# Hello World

This is a simple example.
"#,
            r#"# Project Documentation

## Overview

This project provides a comprehensive solution for data processing.

## Installation

```bash
npm install my-package
```

## Usage

```javascript
const pkg = require('my-package');
pkg.process(data);
```

## API Reference

### `process(data)`

Processes the input data and returns transformed results.

| Parameter | Type | Description |
|-----------|------|-------------|
| data | Array | Input data to process |

**Returns:** Object with processed results.

## Contributing

1. Fork the repository
2. Create your feature branch
3. Submit a pull request

## License

MIT
"#,
        ],
    },
    LanguageTemplate {
        id: "plain",
        samples: &[
            "Just some plain text content.\n",
            "Meeting notes from 2024-01-15:\n\n- Discussed project timeline\n- Reviewed budget constraints\n- Assigned tasks to team members\n- Next meeting scheduled for Friday\n",
            "TODO:\n[ ] Complete documentation\n[ ] Write unit tests\n[ ] Deploy to staging\n[x] Code review\n[x] Fix linting errors\n",
        ],
    },
    LanguageTemplate {
        id: "yaml",
        samples: &[
            "name: example\nversion: 1.0.0\n",
            r#"apiVersion: apps/v1
kind: Deployment
metadata:
  name: my-app
  labels:
    app: my-app
spec:
  replicas: 3
  selector:
    matchLabels:
      app: my-app
  template:
    metadata:
      labels:
        app: my-app
    spec:
      containers:
      - name: my-app
        image: my-app:latest
        ports:
        - containerPort: 8080
        resources:
          limits:
            memory: "128Mi"
            cpu: "500m"
"#,
        ],
    },
    LanguageTemplate {
        id: "toml",
        samples: &[
            "[package]\nname = \"example\"\nversion = \"0.1.0\"\n",
            r#"[package]
name = "my-project"
version = "0.1.0"
edition = "2021"
authors = ["Developer <dev@example.com>"]
description = "A sample project"

[dependencies]
serde = { version = "1.0", features = ["derive"] }
tokio = { version = "1.0", features = ["full"] }

[dev-dependencies]
criterion = "0.5"

[features]
default = []
experimental = ["feature-a", "feature-b"]

[[bin]]
name = "cli"
path = "src/bin/cli.rs"
"#,
        ],
    },
    LanguageTemplate {
        id: "shell",
        samples: &[
            "#!/bin/bash\necho \"Hello, world!\"\n",
            r#"#!/bin/bash
set -euo pipefail

# Configuration
LOG_FILE="/var/log/backup.log"
BACKUP_DIR="/backup"
RETENTION_DAYS=30

log() {
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] $1" | tee -a "$LOG_FILE"
}

cleanup_old_backups() {
    log "Cleaning up backups older than $RETENTION_DAYS days"
    find "$BACKUP_DIR" -type f -mtime +$RETENTION_DAYS -delete
}

create_backup() {
    local timestamp=$(date '+%Y%m%d_%H%M%S')
    local backup_file="$BACKUP_DIR/backup_$timestamp.tar.gz"

    log "Creating backup: $backup_file"
    tar -czf "$backup_file" /data
    log "Backup complete"
}

main() {
    log "Starting backup process"
    create_backup
    cleanup_old_backups
    log "Backup process finished"
}

main "$@"
"#,
        ],
    },
    LanguageTemplate {
        id: "sql",
        samples: &[
            "SELECT * FROM users;\n",
            r#"-- Create users table
CREATE TABLE users (
    id SERIAL PRIMARY KEY,
    username VARCHAR(50) UNIQUE NOT NULL,
    email VARCHAR(100) UNIQUE NOT NULL,
    password_hash VARCHAR(255) NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Create posts table
CREATE TABLE posts (
    id SERIAL PRIMARY KEY,
    user_id INTEGER REFERENCES users(id) ON DELETE CASCADE,
    title VARCHAR(200) NOT NULL,
    content TEXT,
    published BOOLEAN DEFAULT FALSE,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Create index for faster queries
CREATE INDEX idx_posts_user_id ON posts(user_id);
CREATE INDEX idx_posts_published ON posts(published) WHERE published = TRUE;

-- Sample query
SELECT u.username, COUNT(p.id) as post_count
FROM users u
LEFT JOIN posts p ON u.id = p.user_id AND p.published = TRUE
GROUP BY u.id, u.username
ORDER BY post_count DESC
LIMIT 10;
"#,
        ],
    },
];

/// Folder name components for generating realistic folder names.
const FOLDER_PREFIXES: &[&str] = &[
    "project",
    "work",
    "personal",
    "archive",
    "drafts",
    "snippets",
    "notes",
    "config",
    "scripts",
    "examples",
    "templates",
    "backup",
    "temp",
    "shared",
    "private",
];

const FOLDER_SUFFIXES: &[&str] = &[
    "2024", "main", "dev", "prod", "test", "old", "new", "v2", "final", "wip", "review", "approved",
];

fn generate_folder_name(rng: &mut impl Rng) -> String {
    let prefix = FOLDER_PREFIXES.choose(rng).unwrap();
    if rng.gen_bool(0.5) {
        let suffix = FOLDER_SUFFIXES.choose(rng).unwrap();
        format!("{}-{}", prefix, suffix)
    } else {
        (*prefix).to_string()
    }
}

/// Generate content of a specific size category.
fn generate_content(rng: &mut impl Rng, size_category: &str) -> (String, &'static str) {
    let lang = LANGUAGES.choose(rng).unwrap();
    let base_sample = *lang.samples.choose(rng).unwrap();

    let target_size = match size_category {
        "small" => rng.gen_range(100..1024),                  // <1KB
        "medium" => rng.gen_range(1024..10 * 1024),           // 1-10KB
        "large" => rng.gen_range(10 * 1024..50 * 1024),       // 10-50KB
        "very_large" => rng.gen_range(50 * 1024..256 * 1024), // 50-256KB
        _ => rng.gen_range(100..1024),
    };

    let mut content = String::with_capacity(target_size);

    // Build content by repeating and varying the sample
    while content.len() < target_size {
        content.push_str(base_sample);
        // Add some variation
        if rng.gen_bool(0.3) {
            content.push_str("\n// Generated test content\n");
        }
        if rng.gen_bool(0.2) {
            content.push_str(&format!("// Line {}\n", content.lines().count()));
        }
    }

    // Trim to target size at a line boundary if possible
    if content.len() > target_size {
        if let Some(pos) = content[..target_size].rfind('\n') {
            content.truncate(pos + 1);
        } else {
            content.truncate(target_size);
        }
    }

    (content, lang.id)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Initialize tracing for info output
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .init();

    let config = Config::from_env();
    println!("Using database at: {}", config.db_path);

    let db = Database::new(&config.db_path)?;

    if args.clear {
        println!("Clearing existing data...");
        // Delete all pastes (loop until empty to handle large datasets)
        let mut deleted_pastes = 0usize;
        loop {
            let pastes = db.pastes.list(10000, None)?;
            if pastes.is_empty() {
                break;
            }
            for paste in &pastes {
                db.pastes.delete(&paste.id)?;
            }
            deleted_pastes += pastes.len();
        }
        // Delete all folders
        let folders = db.folders.list()?;
        for folder in &folders {
            db.folders.delete(&folder.id)?;
        }
        db.flush()?;
        println!(
            "Cleared {} pastes and {} folders",
            deleted_pastes,
            folders.len()
        );
    }

    let mut rng = rand::thread_rng();

    // Generate folders first
    println!("Generating {} folders...", args.folders);
    let mut folder_ids: Vec<String> = Vec::with_capacity(args.folders);

    for i in 0..args.folders {
        let name = format!("{}_{}", generate_folder_name(&mut rng), i);

        // Some folders can have parents (but not too deep)
        let parent_id = if !folder_ids.is_empty() && rng.gen_bool(0.3) {
            // Only pick from first half to avoid deep nesting
            let max_parent_idx = (folder_ids.len() / 2).max(1);
            Some(folder_ids[rng.gen_range(0..max_parent_idx)].clone())
        } else {
            None
        };

        let folder = Folder::with_parent(name, parent_id);
        db.folders.create(&folder)?;
        folder_ids.push(folder.id);
    }

    println!("Generating {} pastes...", args.count);
    let start = Instant::now();

    // Size distribution: 10% small (<1KB), 70% medium (1-10KB),
    // 15% large (10-50KB), 5% very large (50-256KB)
    for i in 0..args.count {
        // Pick size category based on weights
        let roll: u32 = rng.gen_range(0..100);
        let size_category = if roll < 10 {
            "small"
        } else if roll < 80 {
            "medium"
        } else if roll < 95 {
            "large"
        } else {
            "very_large"
        };

        let (content, language) = generate_content(&mut rng, size_category);
        let name = localpaste_core::naming::generate_name();

        let mut paste = Paste::new(content, name);
        paste.language = Some(language.to_string());
        paste.language_is_manual = rng.gen_bool(0.3); // 30% have manual language set

        // 70% of pastes go into folders
        if !folder_ids.is_empty() && rng.gen_bool(0.7) {
            paste.folder_id = Some(folder_ids.choose(&mut rng).unwrap().clone());
        }

        // Some pastes have tags
        if rng.gen_bool(0.4) {
            let num_tags = rng.gen_range(1..=3);
            let tags = [
                "todo",
                "important",
                "review",
                "wip",
                "done",
                "bug",
                "feature",
            ];
            paste.tags = tags
                .choose_multiple(&mut rng, num_tags)
                .map(|s| s.to_string())
                .collect();
        }

        db.pastes.create(&paste)?;

        // Progress reporting
        if (i + 1) % args.progress_interval == 0 {
            let elapsed = start.elapsed().as_secs_f64();
            let rate = (i + 1) as f64 / elapsed;
            println!(
                "  Created {}/{} pastes ({:.1}/sec)",
                i + 1,
                args.count,
                rate
            );
        }
    }

    db.flush()?;

    let elapsed = start.elapsed();
    let rate = args.count as f64 / elapsed.as_secs_f64();

    println!("\nGeneration complete:");
    println!("  Folders: {}", args.folders);
    println!("  Pastes:  {}", args.count);
    println!("  Time:    {:.2}s", elapsed.as_secs_f64());
    println!("  Rate:    {:.1} pastes/sec", rate);
    println!("\nDatabase path: {}", config.db_path);

    Ok(())
}
