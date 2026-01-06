# Shell Utils

A collection of high-performance CLI tools written in Rust for common shell operations.

## Tools

### check-zip

A fast, multi-threaded tool for checking the integrity of ZIP archives and detecting password-protected files.

#### Features

- 🚀 **Multi-threaded processing** - Automatically uses all available CPU cores for maximum performance
- ✅ **Integrity validation** - Verifies ZIP archive structure and file accessibility
- 🔒 **Password detection** - Identifies encrypted/password-protected archives
- 📁 **Recursive scanning** - Automatically scans directories for ZIP files
- 🎯 **Clear output** - Visual indicators for different file states

#### Installation

##### From Source

```bash
git clone https://github.com/yourusername/shell-utils.git
cd shell-utils
cargo build --release
```

The binary will be available at `target/release/check-zip`.

##### Install with Cargo

```bash
cargo install --path cz
```

#### Usage

Check ZIP files in the current directory:

```bash
check-zip
```

Check ZIP files in a specific directory:

```bash
check-zip --path /path/to/directory
# or
check-zip -p /path/to/directory
```

#### Output Examples

The tool provides clear visual feedback for each ZIP file:

```
✓ Valid: /path/to/file.zip
🔒 Password protected: /path/to/encrypted.zip
✗ Corrupted (Invalid zip format: ...): /path/to/broken.zip
```

#### Output Indicators

- **✓ Valid** - ZIP archive is valid and accessible
- **🔒 Password protected** - ZIP archive contains encrypted files
- **✗ Corrupted** - ZIP archive is damaged or unreadable
- **⚠ Unsupported** - ZIP format is not supported

#### Performance

The tool automatically detects and uses all available CPU cores for parallel processing, making it extremely fast for
scanning large directories with many ZIP files.

## Requirements

- Rust 1.70 or higher (2024 edition)

## Building

Build all tools:

```bash
cargo build --release
```

Build a specific tool:

```bash
cargo build --release -p cz
```

## Development

### Running Tests

```bash
cargo test
```

### Running with Debug Output

```bash
cargo run -p cz -- --path /path/to/directory
```

## Project Structure

```
shell-utils/
├── cz/                    # check-zip tool
│   ├── src/
│   │   ├── main.rs       # Main application logic
│   │   └── cli.rs        # CLI argument parsing
│   └── Cargo.toml
├── Cargo.toml            # Workspace configuration
└── README.md
```

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

This project is licensed under the MIT License - see the LICENSE file for details.

## Roadmap

- [ ] Add support for other archive formats (tar, rar, 7z)
- [ ] Add repair functionality for corrupted archives
- [ ] Add detailed statistics and reporting
- [ ] Add JSON output format
- [ ] Add more shell utilities

## Author

HUANG Cheng

## Acknowledgments

- Built with [clap](https://github.com/clap-rs/clap) for CLI parsing
- ZIP handling powered by [zip-rs](https://github.com/zip-rs/zip2)

