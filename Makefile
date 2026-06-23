# LanType Makefile
#   make release  — build + auto-package for current platform
#   make clean    — remove build artifacts

.PHONY: release clean

release:
	cargo build --release
ifeq ($(shell uname -s),Darwin)
	@echo "==> macOS detected, creating .app bundle..."
	@rm -rf target/release/LanType.app
	@mkdir -p target/release/LanType.app/Contents/MacOS
	@mkdir -p target/release/LanType.app/Contents/Resources
	@cp target/release/lantype target/release/LanType.app/Contents/MacOS/
	@chmod +x target/release/LanType.app/Contents/MacOS/lantype
	@cp Info.plist target/release/LanType.app/Contents/
	@cp icons/icon.icns target/release/LanType.app/Contents/Resources/
	@echo "==> Done: target/release/LanType.app"
	@echo "    open target/release/LanType.app"
else ifeq ($(OS),Windows_NT)
	@echo "==> Windows: icon already embedded in .exe, no extra step needed."
	@echo "    target\\release\\lantype.exe"
else
	@echo "==> Linux: binary ready at target/release/lantype"
endif

clean:
	cargo clean
	rm -rf target/release/LanType.app