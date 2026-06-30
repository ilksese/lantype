# LanType Makefile
#   make release          — build + auto-package for current platform
#   make release-all      — build macOS + Windows release artifacts
#   make release-macos    — build macOS universal .app bundle
#   make release-windows  — cross-compile Windows GNU .exe
#   make clean            — remove build artifacts

.PHONY: release release-all release-macos release-windows clean phone

MACOS_TARGET := universal2-apple-darwin
WINDOWS_TARGET := x86_64-pc-windows-gnu

release: phone
	cargo build --release
ifeq ($(shell uname -s),Darwin)
	./package.sh
else ifeq ($(OS),Windows_NT)
	@echo "==> Windows: icon already embedded in .exe, no extra step needed."
	@echo "    target\\release\\lantype.exe"
else
	@echo "==> Linux: binary ready at target/release/lantype"
endif

release-all: release-macos release-windows

release-macos: phone
	@echo "==> Building macOS universal binary with cargo-zigbuild..."
	cargo zigbuild --release --target $(MACOS_TARGET)
	./package.sh $(MACOS_TARGET)

release-windows: phone
	@echo "==> Cross-compiling Windows GNU binary with cargo-zigbuild..."
	cargo zigbuild --release --target $(WINDOWS_TARGET)
	@echo "==> Done: target/$(WINDOWS_TARGET)/release/lantype.exe"

phone:
	@echo "==> Building phone page (Preact + Vite)..."
	@npm --prefix web/phone run build

clean:
	cargo clean
	rm -rf target/release/LanType.app target/$(MACOS_TARGET)/release/LanType.app
