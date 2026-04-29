# Harness top-level dev shortcuts.
# All real work happens in cargo / swift / xcodebuild — this file just orchestrates.

.PHONY: build test backend app uitest fmt lint clean help

help:
	@echo "Targets:"
	@echo "  build       Build backend (debug) and Swift package"
	@echo "  test        Run Rust + Swift tests"
	@echo "  backend     Run the Rust backend in dev mode"
	@echo "  app         Build and run the macOS app (requires xcodegen)"
	@echo "  uitest      Run XCUITest e2e suite against dist/Harness.app (requires xcodebuild)"
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
	@command -v xcodegen >/dev/null 2>&1 || { echo "xcodegen not found. Install with: brew install xcodegen (or set XCODEGEN=/path/to/xcodegen)"; exit 1; }
	./scripts/package.sh
	./scripts/test-app.sh
	open dist/Harness.app

# UI tests are slow (full app launch per case), so they live behind a
# separate target rather than `make test`. Drives xcodebuild against the
# HarnessUITests scheme; assumes `make app` has produced dist/Harness.app
# so HARNESS_TEST_APP_PATH resolves cleanly.
uitest:
	@command -v xcodegen >/dev/null 2>&1 || { echo "xcodegen not found. Install with: brew install xcodegen (or set XCODEGEN=/path/to/xcodegen)"; exit 1; }
	./scripts/package.sh
	cd macos && xcodegen generate
	cd macos && xcodebuild test \
	    -project Harness.xcodeproj \
	    -scheme HarnessUITests \
	    -destination 'platform=macOS' \
	    -derivedDataPath build

fmt:
	cd backend && cargo fmt --all
	cd macos && swift format -i -r Sources Tests 2>/dev/null || true

lint:
	cd backend && cargo clippy --workspace --all-targets -- -D warnings

clean:
	cd backend && cargo clean
	cd macos && rm -rf .build .swiftpm DerivedData
	rm -rf dist out
