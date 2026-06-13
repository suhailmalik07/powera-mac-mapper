.PHONY: app

APP_NAME := powerA Mapper

# Build the GUI mapper, then run it under a friendly executable name so it shows
# as "powerA Mapper" in Activity Monitor and the macOS app menu (Cargo binary
# names can't contain spaces, so we copy the built binary to a nice name).
app:
	cargo build --release --bin app
	@cp -f target/release/app "target/release/$(APP_NAME)"
	@"target/release/$(APP_NAME)"
