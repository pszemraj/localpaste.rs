# Development Documentation

## Project Structure

```
localpaste.rs/
├── src/
│   ├── main.rs           # Application entry point
│   ├── config.rs         # Configuration management
│   ├── error.rs          # Error types
│   ├── naming/           # Semantic name generation
│   ├── models/           # Data models
│   │   ├── paste.rs      # Paste model and requests
│   │   └── folder.rs     # Folder model
│   ├── db/               # Database layer
│   │   ├── paste.rs      # Paste CRUD operations
│   │   └── folder.rs     # Folder CRUD operations
│   ├── handlers/         # HTTP handlers
│   │   ├── paste.rs      # Paste endpoints
│   │   └── folder.rs     # Folder endpoints
│   ├── static/           # Frontend assets (embedded)
│   │   ├── index.html    # Main UI
│   │   └── css/          # Styles
│   └── bin/
│       └── lpaste.rs     # CLI tool
├── ~/.cache/localpaste/  # Runtime data (user cache directory)
│   └── db/               # Sled database files
└── target/               # Build artifacts (git-ignored)
```

## Key Design Decisions

### Single Binary Distribution
- All static assets are embedded using `rust-embed`
- No external dependencies at runtime
- Database is embedded (Sled)

### Database Choice
- **Sled**: Embedded, ACID-compliant, fast
- No external database server required
- Data stored in `~/.cache/localpaste/db/`

### Frontend Architecture
- Vanilla JavaScript (no framework dependencies)
- Language detection for syntax awareness
- Drag & drop for folder organization
- Auto-save with debouncing

### API Design
RESTful endpoints:
- `POST /api/paste` - Create paste
- `GET /api/paste/:id` - Get paste
- `PUT /api/paste/:id` - Update paste
- `DELETE /api/paste/:id` - Delete paste
- `GET /api/pastes` - List pastes
- `GET /api/search?q=` - Search pastes
- `POST /api/folder` - Create folder
- `GET /api/folders` - List folders
- `DELETE /api/folder/:id` - Delete folder

## Development Workflow

### Building the Project

The project contains two binaries:
- `localpaste` - The web server (default binary)
- `lpaste` - CLI tool for interacting with the server

```bash
# Build both binaries
cargo build --release

# Build only the server
cargo build --release --bin localpaste

# Build only the CLI
cargo build --release --bin lpaste
```

### Running Locally
```bash
# Run the server (main application)
cargo run --bin localpaste --release

# Run with auto-reload (requires cargo-watch)
cargo install cargo-watch
cargo watch -x "run --bin localpaste"

# Run with debug logging
RUST_LOG=debug cargo run --bin localpaste

# Run tests
cargo test

# Format code
cargo fmt

# Lint
cargo clippy
```

### Building for Production
```bash
# Optimized build (both binaries)
cargo build --release

# Strip symbols for smaller binaries
strip target/release/localpaste
strip target/release/lpaste

# Check binary sizes
du -h target/release/localpaste target/release/lpaste
```

### Using the CLI Tool
```bash
# The CLI tool should be built and run from the binary, not cargo run
./target/release/lpaste --help

# Examples
echo "test" | ./target/release/lpaste new
./target/release/lpaste list
./target/release/lpaste get <paste-id>
```

### Adding New Features

1. **Backend Changes**
   - Add model in `src/models/`
   - Add database operations in `src/db/`
   - Add handler in `src/handlers/`
   - Register route in `src/main.rs`

2. **Frontend Changes**
   - Edit `src/static/index.html`
   - Rebuild with `cargo build --release`
   - Assets are embedded automatically

3. **Database Migrations**
   - Sled handles schema evolution automatically
   - Add migration logic in `src/db/mod.rs` if needed

## Code Style

- Use `cargo fmt` before committing
- Follow Rust naming conventions
- Keep functions small and focused
- Add doc comments for public APIs

## Testing

```bash
# Run all tests
cargo test

# Run specific test
cargo test test_name

# Run with output
cargo test -- --nocapture

# Run benchmarks
cargo bench
```

## Debugging

### Enable Debug Logging
```bash
RUST_LOG=debug cargo run
```

### Database Inspection
```bash
# View database files
ls -la ~/.cache/localpaste/db/

# Database size
du -sh ~/.cache/localpaste/
```

### Common Issues

**Database Lock Error**
- Kill any running instances: `pkill -f localpaste`
- Remove lock file if stuck: `rm ~/.cache/localpaste/db/db.lock`

**Port Already in Use**
- Check what's using port: `lsof -i :3030`
- Change port: `PORT=3031 cargo run`

## Performance Optimization

- Release builds use `opt-level = "z"` for size
- LTO enabled for better optimization
- Single codegen unit for smaller binary
- Gzip compression for HTTP responses
- Embedded assets are compressed

## Security

See [security.md](security.md) for detailed security configuration and best practices.

## Contributing

1. Format code: `cargo fmt`
2. Check lints: `cargo clippy`
3. Run tests: `cargo test`
4. Update documentation if needed
5. Create descriptive commit messages