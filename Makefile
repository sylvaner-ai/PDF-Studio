# PDF Studio — commandes de build.
#
#   make build     application complète : build/PDF Studio.app
#   make test      tests du moteur (cargo) puis de l'application (XCTest)
#   make lint      clippy, rustfmt et SwiftLint
#   make bench     banc d'essai des moteurs de rendu (cf. docs/RENDER_BENCHMARK.md)
#   make ocr-bench banc d'essai des moteurs OCR (cf. docs/OCR_BENCHMARK.md)
#   make fixtures  régénère le jeu de PDF de test
#   make icon      régénère les images de l'icône depuis app/Resources/AppIcon.svg
#   make snapshots captures d'écran de l'interface, en mode clair et sombre
#   make perf      mesures de performance sur le Mac (cf. docs/PERFORMANCES.md)
#
# Le chemin du dépôt peut contenir des espaces et se trouver dans un dossier
# synchronisé par iCloud Drive. Deux conséquences :
# - la compilation de MuPDF échoue si son dossier de sortie contient un espace ;
# - iCloud ajoute aux fichiers des attributs étendus que la signature refuse.
# Les produits intermédiaires (cible Cargo, DerivedData) sont donc placés dans
# PDFSTUDIO_BUILD_ROOT, hors du dépôt et sans espace ; seule l'application
# finale est copiée dans build/.

SHELL := /bin/bash
export PATH := $(HOME)/.cargo/bin:/opt/homebrew/bin:$(PATH)

PDFSTUDIO_BUILD_ROOT ?= $(HOME)/Library/Caches/PDFStudio
export CARGO_TARGET_DIR := $(PDFSTUDIO_BUILD_ROOT)/target

RUST_TARGET   := aarch64-apple-darwin
CARGO         := cargo
CARGO_FLAGS   := --manifest-path engine/Cargo.toml
ENGINE_DIR    := $(CARGO_TARGET_DIR)/$(RUST_TARGET)/release
GENERATED     := app/Bridge/Generated
XCODE_PROJECT := app/PDFStudio.xcodeproj
DERIVED_DATA  := $(PDFSTUDIO_BUILD_ROOT)/DerivedData
APP_NAME      := PDF Studio.app
PDFIUM_URL    := https://github.com/bblanchon/pdfium-binaries/releases/latest/download/pdfium-mac-arm64.tgz
PDFIUM_DIR    := .tools/pdfium

.PHONY: build engine bindings project test test-engine test-app lint bench ocr-bench fixtures icon snapshots perf clean

build: project
	xcodebuild -project "$(XCODE_PROJECT)" -scheme PDFStudio -configuration Release \
		-derivedDataPath "$(DERIVED_DATA)" -destination 'platform=macOS,arch=arm64' build
	mkdir -p build
	rm -rf "build/$(APP_NAME)"
	ditto "$(DERIVED_DATA)/Build/Products/Release/$(APP_NAME)" "build/$(APP_NAME)"
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
		"$(ENGINE_DIR)/libpdfstudio_ffi.dylib" "../$(GENERATED)" \
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
# l'application ne l'utilise pas. Les documents privés sont anonymisés dans le
# rapport ; les images de comparaison, qui montrent leur contenu, sont écrites
# hors du dépôt et hors d'iCloud, dans BENCH_OUT, vidé à chaque exécution.
BENCH_OUT := $(PDFSTUDIO_BUILD_ROOT)/bench

bench:
	@if [ ! -f "$(PDFIUM_DIR)/lib/libpdfium.dylib" ]; then \
		mkdir -p "$(PDFIUM_DIR)" && curl -sSfL "$(PDFIUM_URL)" | tar -xz -C "$(PDFIUM_DIR)"; fi
	$(CARGO) build $(CARGO_FLAGS) --release -p render_bench
	rm -rf "$(BENCH_OUT)"
	"$(CARGO_TARGET_DIR)/release/render_bench" --pdfium "$(PDFIUM_DIR)/lib" --out "$(BENCH_OUT)/public" \
		$(BENCH_FLAGS) tests/fixtures
	@if ls tests/fixtures/private/*.pdf >/dev/null 2>&1; then \
		"$(CARGO_TARGET_DIR)/release/render_bench" --pdfium "$(PDFIUM_DIR)/lib" --out "$(BENCH_OUT)/private" \
			--anonymize $(BENCH_FLAGS) tests/fixtures/private; fi

# Banc d'essai OCR : Vision, Tesseract et ocrs, sur des scans simulés à partir
# de documents dont le texte est connu (cf. docs/OCR_BENCHMARK.md). Tesseract
# (Homebrew), ses modèles français et anglais et ceux d'ocrs ne servent qu'à
# cette mesure : l'application n'utilise que Vision. Vision est appelé par un
# petit programme qui compile le code de l'application. Les documents privés
# n'apparaissent dans le rapport que sous un nom anonyme, avec leurs mesures.
OCR_OUT      := $(PDFSTUDIO_BUILD_ROOT)/ocr-bench
OCRS_MODELS  := https://ocrs-models.s3-accelerate.amazonaws.com
TESSDATA_URL := https://github.com/tesseract-ocr/tessdata/raw/main
OCR_DOCUMENTS := tests/fixtures/ocr tests/fixtures/texte_simple.pdf tests/fixtures/tableau.pdf \
	tests/fixtures/edition.pdf tests/fixtures/encadre.pdf tests/fixtures/scan_simule.pdf

ocr-bench:
	@command -v tesseract >/dev/null || brew install tesseract
	mkdir -p "$(OCR_OUT)/ocrs" "$(OCR_OUT)/tessdata"
	@for model in text-detection text-recognition; do \
		[ -f "$(OCR_OUT)/ocrs/$$model.rten" ] || \
			curl -sSfL "$(OCRS_MODELS)/$$model.rten" -o "$(OCR_OUT)/ocrs/$$model.rten"; done
	@for lang in fra eng osd; do \
		[ -f "$(OCR_OUT)/tessdata/$$lang.traineddata" ] || \
			curl -sSfL "$(TESSDATA_URL)/$$lang.traineddata" -o "$(OCR_OUT)/tessdata/$$lang.traineddata"; done
	swiftc -O -o "$(OCR_OUT)/vision_ocr" app/Recognition/VisionTextReader.swift \
		engine/tools/ocr_bench/vision/main.swift
	$(CARGO) build $(CARGO_FLAGS) --release -p ocr_bench
	"$(CARGO_TARGET_DIR)/release/ocr_bench" --vision "$(OCR_OUT)/vision_ocr" \
		--tessdata "$(OCR_OUT)/tessdata" --ocrs-models "$(OCR_OUT)/ocrs" \
		--out "$(OCR_OUT)/rapport.md" --csv "$(OCR_OUT)/detail.csv" $(OCR_BENCH_FLAGS) $(OCR_DOCUMENTS) \
		$$(ls tests/fixtures/private/*.pdf >/dev/null 2>&1 && echo --private tests/fixtures/private)
	@echo "Rapport : $(OCR_OUT)/rapport.md"

fixtures:
	python3 tests/fixtures/generate.py

# Images de l'icône (jeu d'icônes de macOS), depuis sa source SVG ; demande
# resvg-py et Pillow (pip install resvg-py pillow). Les images sont versionnées :
# la compilation n'en a pas besoin.
icon:
	python3 tools/icon/render_icon.py

# Captures d'écran de l'interface sur le jeu de test, en mode clair et sombre
# (cf. app/Tests/DesignSnapshotTests.swift, et WindowTabsTests.swift pour la
# barre d'onglets). Le test écrit dans les caches du bac
# à sable de l'application ; les images sont copiées hors d'iCloud, dans
# SNAPSHOTS_OUT. Les fenêtres apparaissent quelques secondes à l'écran.
SNAPSHOTS_OUT     := $(PDFSTUDIO_BUILD_ROOT)/snapshots
SNAPSHOTS_SANDBOX := $(HOME)/Library/Containers/fr.sylvainrobert.pdfstudio/Data/Library/Caches/Snapshots

snapshots: project
	TEST_RUNNER_PDFSTUDIO_SNAPSHOTS=1 xcodebuild -project "$(XCODE_PROJECT)" -scheme PDFStudio \
		-configuration Debug -derivedDataPath "$(DERIVED_DATA)" -destination 'platform=macOS,arch=arm64' \
		test -only-testing:PDFStudioTests/DesignSnapshotTests \
		-only-testing:PDFStudioTests/WindowTabsTests
	rm -rf "$(SNAPSHOTS_OUT)"
	cp -R "$(SNAPSHOTS_SANDBOX)" "$(SNAPSHOTS_OUT)"
	@echo "Captures : $(SNAPSHOTS_OUT)"

# Mesures de performance du cahier des charges (§6) : ouverture, défilement,
# reconnaissance du texte, retouche d'un scan, récupération d'un gros document
# (cf. app/Tests/PerformanceTests.swift). Le code Swift est compilé optimisé
# (-O), comme dans la version Release ; le moteur l'est toujours. Chaque mesure
# écrit son tableau dans les caches du bac à sable ; ils sont réunis dans
# PERF_OUT/rapport.md. Les fenêtres apparaissent quelques secondes à l'écran.
PERF_OUT     := $(PDFSTUDIO_BUILD_ROOT)/perf
PERF_SANDBOX := $(HOME)/Library/Containers/fr.sylvainrobert.pdfstudio/Data/Library/Caches/Performances

perf: project
	rm -rf "$(PERF_SANDBOX)"
	TEST_RUNNER_PDFSTUDIO_PERF=1 xcodebuild -project "$(XCODE_PROJECT)" -scheme PDFStudio \
		-configuration Debug -derivedDataPath "$(DERIVED_DATA)/Perf" -destination 'platform=macOS,arch=arm64' \
		SWIFT_OPTIMIZATION_LEVEL=-O test -only-testing:PDFStudioTests/PerformanceTests
	mkdir -p "$(PERF_OUT)"
	cat "$(PERF_SANDBOX)"/*.md > "$(PERF_OUT)/rapport.md"
	@echo "Rapport : $(PERF_OUT)/rapport.md"

clean:
	$(CARGO) clean $(CARGO_FLAGS)
	rm -rf build "$(GENERATED)" "$(XCODE_PROJECT)" "$(DERIVED_DATA)"
