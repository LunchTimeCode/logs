# Agent Guidelines for Logs Project

## Build/Test Commands
- `cargo build` - Build the project
- `cargo run` - Run the application
- `cargo test` - Run all tests
- `cargo test <test_name>` - Run a specific test
- `cargo check` - Fast check for compilation errors
- `cargo clippy` - Lint with Clippy

## Project Overview
This is a Rust GUI application for log viewing with filtering and search capabilities using egui/eframe and egui_logger. Uses xshell for CLI integration.

## Code Style Guidelines
- Use Rust 2024 edition conventions
- Follow standard Rust naming: snake_case for functions/variables, PascalCase for types
- Use explicit types where clarity is needed, prefer type inference elsewhere
- Handle errors with `Result<T, E>` and `?` operator
- Use `cargo fmt` for consistent formatting
- Prefer immutable variables unless mutation is necessary
- Document public APIs with `///` comments
- Use `unwrap()` sparingly, prefer proper error handling