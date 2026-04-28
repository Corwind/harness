# Harness top-level dev shortcuts.
# All real work happens in cargo / swift / xcodebuild — this file just orchestrates.

.PHONY: build test backend app fmt lint clean help

help:
	@echo "Targets:"
	@echo "  build       Build backend (debug) and Swift package"
	@echo "  test        Run Rust + Swift tests"
	@echo "  backend     Run the Rust backend in dev mode"
	@echo "  app         Build and run the macOS app (requires xcodegen)"
	@echo "  fmt         Format Rust + Swift sources"
	@echo "  lint        Clippy + SwiftLint (when configured)"
	@echo "  clean       Remove build artifacts"

build:
	cd backend && cargo build
	cd macos && swift build

test:
	cd backend && cargo test --workspace
	cd macos && swift test

backend:
	cd backend && cargo run -p harness-server

app:
	@command -v xcodegen >/dev/null 2>&1 || { echo "xcodegen not found. Install with: brew install xcodegen"; exit 1; }
	cd macos && xcodegen generate
	cd macos && xcodebuild -scheme Harness -configuration Debug build
	open macos/build/Debug/Harness.app

fmt:
	cd backend && cargo fmt --all
	cd macos && swift format -i -r Sources Tests 2>/dev/null || true

lint:
	cd backend && cargo clippy --workspace --all-targets -- -D warnings

clean:
	cd backend && cargo clean
	cd macos && rm -rf .build .swiftpm DerivedData
	rm -rf dist out
