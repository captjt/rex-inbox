# rex-inbox's development tasks. `just` lists them; see CONTRIBUTING.md.

set shell := ["bash", "-euo", "pipefail", "-c"]

# A hub of your own for trying changes, apart from your everyday one: its
# own state, socket, and web port, and no Mac notifications.
dev_env := "REX_INBOX_STATE=/tmp/rex-inbox-dev REX_INBOX_SOCKET=/tmp/rex-inbox-dev.sock REX_INBOX_HTTP=127.0.0.1:7440 REX_INBOX_NOTIFY=off"

# List the tasks
default:
    @just --list --unsorted

# Build the hub and CLI (one binary)
[group('build')]
build:
    cargo build

# Install this build as your everyday rex-inbox, and restart the hub on it
[group('build')]
install:
    cargo install --path .
    rex-inbox up

# Build Rex Inbox.app into ~/Applications, and restart it
[group('build')]
app:
    mac/build.sh

# Everything for a working setup here: rex-inbox, the hub, the agents' hooks and skill, and the app
[group('build')]
setup: install app
    rex-inbox hooks install
    rex-inbox skill

# Every check a pull request needs
[group('check')]
check: fmt-check lint test swift web

# Format the Rust code (rustfmt.toml)
[group('check')]
fmt:
    cargo fmt

# Fail if the Rust code isn't formatted
[group('check')]
fmt-check:
    cargo fmt --check

# Clippy, with warnings as errors
[group('check')]
lint:
    cargo clippy --all-targets -- -D warnings

# Run the Rust tests; pass a name to run some
[group('check')]
test *filter:
    cargo test {{ filter }}

# Build the Mac app, to check it compiles
[group('check')]
swift:
    swift build --package-path mac

# Check the phone app's scripts parse
[group('check')]
web:
    node --check web/app.js
    node --check web/sw.js

# Run a hub of your own in the foreground (state in /tmp/rex-inbox-dev, web on :7440)
[group('dev')]
dev-hub:
    {{ dev_env }} cargo run -- hub

# Run the CLI against your own hub: just dev status, just dev agents
[group('dev')]
dev *args:
    {{ dev_env }} cargo run -q -- {{ args }}

# Forget your own hub's state
[group('dev')]
dev-reset:
    rm -rf /tmp/rex-inbox-dev

# Serve the made-up world (fake Rex, stand-in agents), with web/ from disk, at http://127.0.0.1:7439
[group('dev')]
world:
    scripts/screenshots.sh --serve

# Fill your own inbox with a made-up project of real Rex panes
[group('dev')]
demo-up:
    scripts/demo.sh up

# Remove the made-up project from your inbox
[group('dev')]
demo-down:
    scripts/demo.sh down

# Render the Mac app to a PNG: just snapshot out.png --tab agents --appearance dark
[group('dev')]
snapshot out *args:
    swift build -c release --package-path mac
    mac/.build/release/RexInbox --snapshot {{ out }} {{ args }}

# Shoot a page at phone size: just phone-shot http://127.0.0.1:7439/ out.png dark
[group('dev')]
phone-shot url out look="light":
    node scripts/phone-shot.mjs {{ url }} {{ out }} {{ look }}

# Render the README's screenshots and the social preview from the made-up world
[group('docs')]
screenshots:
    scripts/screenshots.sh

# Your everyday hub: whether it's running, and what it follows
[group('hub')]
status:
    rex-inbox status

# The end of your everyday hub's log
[group('hub')]
logs:
    rex-inbox logs

# Start or restart your everyday hub
[group('hub')]
up:
    rex-inbox up

# Stop your everyday hub
[group('hub')]
down:
    rex-inbox down

# Set the version (just bump 0.4.0); then commit, tag and release as CONTRIBUTING.md says
[group('release')]
bump version:
    @[[ "{{ version }}" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "a version like 0.4.0, not {{ version }}"; exit 1; }
    sed -i '' 's/^version = ".*"/version = "{{ version }}"/' Cargo.toml
    cargo build -q
    git diff --stat Cargo.toml Cargo.lock
    @echo 'Next: git commit -am "Version {{ version }}", then tag v{{ version }} and release (CONTRIBUTING.md)'
