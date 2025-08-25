BINNAME := "tale"
RELATIVE_TAP_PATH := "../../../homebrew-tap/"

_help:
	just -l

# Run all tests using nextest.
test:
	cargo nextest run

# Run the same checks we run in CI. Requires nightly.
ci: test
	cargo clippy
	cargo +nightly fmt --check

fmt:
	cargo +nightly fmt

# Ask for clippy's opinion.
lint:
	cargo clippy --fix
	cargo +nightly fmt

# Install required tools
setup:
	brew tap ceejbot/tap
	brew install fzf cargo-nextest tomato semver-bump
	rustup install nightly

# Preview what the next changelog will look like.
changelog-preview BUMP:
	#!/usr/bin/env bash
	set -e
	current=$(tomato get package.version Cargo.toml)
	version=$(semver-bump {{BUMP}} "$current")
	echo "Preview of changelog for v${version}:"
	echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
	git-cliff --tag "v${version}" --unreleased

# Tag a new version for release with changelog update.
version BUMP:
	#!/usr/bin/env bash
	set -e
	current=$(tomato get package.version Cargo.toml)
	version=$(semver-bump {{BUMP}} "$current")

	echo "Preparing release v${version}..."

	# Generate changelog for the new version
	echo "Generating changelog entries..."
	git-cliff --tag "v${version}" --unreleased --prepend CHANGELOG.md

	# Show the generated changelog section for review
	echo "New changelog entries:"
	cat CHANGELOG.md
	echo ""

	# Update version in Cargo.toml
	tomato set package.version "$version" Cargo.toml &> /dev/null
	cargo generate-lockfile

	# Ask for confirmation
	read -p "Does the changelog look good? (y/N): " -n 1 -r
	echo
	if [[ $REPLY =~ ^[Yy]$ ]]; then
		# Commit everything and tag
		git add Cargo.toml Cargo.lock CHANGELOG.md
		git commit -m "v${version}"
		git tag "v${version}"
		echo "Release tagged for version v${version}"
		echo ""
		echo "Next steps:"
		echo "  • Review the tag: git show v${version}"
		echo "  • Push when ready: git push && git push --tags"
		echo "  • Publish on crates.io: cargo publish"
		echo "  • The workflow creates a release here"
	else
		# Revert changes
		echo "❌ Reverting changes..."
		git checkout -- Cargo.toml Cargo.lock CHANGELOG.md
		echo "Version bump cancelled. You can edit CHANGELOG.md manually and run again."
		exit 1
	fi

# Release by hand instead of in action.
release:
	#!/usr/bin/env bash
	set -e

	mkdir -p dist
	cd dist
	tag=$(git describe --tags --abbrev=0)
	# fails if this already exists
	release_url=$(gh release create "$tag" --generate-notes)

	for target in "aarch64-apple-darwin" "x86_64-apple-darwin"; do
		cargo +stable build --release --target $target
		tar czf {{ BINNAME }}-$target.tar.gz --strip-components=2  target/$target/release/{{ BINNAME }}
		gh release upload "$tag" "{{ BINNAME }}-$target.tar.gz"
		sha256sum {{ BINNAME }}-$target.tar.gz > {{ BINNAME }}-"$target".tar.gz.sha256
		gh release upload "$tag" "{{ BINNAME }}-$target.tar.gz.sha256"
	done
	formula_file=$(formulaic ../Cargo.toml)
	mv dist/$formula_file {{RELATIVE_TAP_PATH}}/Formula/
	cd {{RELATIVE_TAP_PATH}} || exit
	git add Formula/$(basename $formula_file)
	git commit -m "$(basename -s .rb $formula_file) $tag"
