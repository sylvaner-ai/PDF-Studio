# PDF Studio — commandes de build.
#
#   make build     application complète : build/PDF Studio.app
#   make test      tests du moteur (cargo) puis de l'application (XCTest)
#   make lint      clippy, rustfmt et SwiftLint
#   make bench     banc d'essai des moteurs de rendu (cf. docs/RENDER_BENCHMARK.md)
#   make fixtures  régénère le jeu de PDF de test
#
# Le chemin du dépôt peut contenir des espaces : toutes les recettes utilisent des
# chemins relatifs et des guillemets.

SHELL := /bin/bash
export PATH := $(HOME)/.cargo/bin:/opt/homebrew/bin:$(PATH)

RUST_TARGET   := aarch64-apple-darwin
CARGO         := cargo
CARGO_FLAGS   := --manifest-path engine/Cargo.toml
ENGINE_DIR    := engine/target/$(RUST_TARGET)/release
GENERATED     := app/Bridge/Generated
XCODE_PROJECT := app/PDFStudio.xcodeproj
DERIVED_DATA  := build/DerivedData
APP_NAME      := PDF Studio.app
PDFIUM_URL    := https://github.com/bblanchon/pdfium-binaries/releases/latest/download/pdfium-mac-arm64.tgz
PDFIUM_DIR    := .tools/pdfium

.PHONY: build engine bindings project test test-engine test-app lint bench fixtures clean

build: project
	xcodebuild -project "$(XCODE_PROJECT)" -scheme PDFStudio -configuration Release \
		-derivedDataPath "$(DERIVED_DATA)" -destination 'platform=macOS,arch=arm64' build
	rm -rf "build/$(APP_NAME)"
	cp -R "$(DERIVED_DATA)/Build/Products/Release/$(APP_NAME)" "build/$(APP_NAME)"
	@echo "Application prête : build/$(APP_NAME)"

# Bibliothèque statique du moteur (MuPDF compris), pour Apple Silicon.
engine:
	$(CARGO) build $(CARGO_FLAGS) --release --target $(RUST_TARGET) -p pdfstudio_ffi

# Liaisons Swift générées depuis la bibliothèque, et copie de la bibliothèque
# statique seule dans app/Bridge/Generated (l'éditeur de liens y trouve ainsi
# le .a et non le .dylib).
bindings: engine
	mkdir -p "$(GENERATED)"
	cd engine && $(CARGO) run --release -q -p uniffi-bindgen --bin uniffi-bindgen-swift -- \
		"target/$(RUST_TARGET)/release/libpdfstudio_ffi.dylib" "../$(GENERATED)" \
		--swift-sources --headers --modulemap \
		--module-name pdfstudio_ffiFFI --modulemap-filename module.modulemap
	cp "$(ENGINE_DIR)/libpdfstudio_ffi.a" "$(GENERATED)/libpdfstudio_ffi.a"

project: bindings
	@command -v xcodegen >/dev/null || { echo "XcodeGen est requis : brew install xcodegen"; exit 1; }
	cd app && xcodegen generate --quiet

test: test-engine test-app

test-engine:
	$(CARGO) test $(CARGO_FLAGS) --workspace

test-app: project
	xcodebuild -project "$(XCODE_PROJECT)" -scheme PDFStudio -configuration Debug \
		-derivedDataPath "$(DERIVED_DATA)" -destination 'platform=macOS,arch=arm64' test

lint:
	$(CARGO) clippy $(CARGO_FLAGS) --workspace --all-targets -- -D warnings
	$(CARGO) fmt $(CARGO_FLAGS) --all -- --check
	@if command -v swiftlint >/dev/null; then cd app && swiftlint lint --strict; \
	else echo "SwiftLint absent : brew install swiftlint"; fi

# Banc d'essai MuPDF / PDFium. PDFium n'est téléchargé que pour cette mesure ;
# l'application ne l'utilise pas. Les documents privés sont anonymisés.
bench:
	@if [ ! -f "$(PDFIUM_DIR)/lib/libpdfium.dylib" ]; then \
		mkdir -p "$(PDFIUM_DIR)" && curl -sSfL "$(PDFIUM_URL)" | tar -xz -C "$(PDFIUM_DIR)"; fi
	$(CARGO) build $(CARGO_FLAGS) --release -p render_bench
	engine/target/release/render_bench --pdfium "$(PDFIUM_DIR)/lib" --out build/bench \
		$(BENCH_FLAGS) tests/fixtures
	@if ls tests/fixtures/private/*.pdf >/dev/null 2>&1; then \
		engine/target/release/render_bench --pdfium "$(PDFIUM_DIR)/lib" --out build/bench-private \
			--anonymize $(BENCH_FLAGS) tests/fixtures/private; fi

fixtures:
	python3 tests/fixtures/generate.py

clean:
	$(CARGO) clean $(CARGO_FLAGS)
	rm -rf build "$(GENERATED)" "$(XCODE_PROJECT)"
