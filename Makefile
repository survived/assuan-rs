docs:
	RUSTDOCFLAGS="--cfg docsrs" cargo +nightly doc --no-deps --all-features

docs-open:
	RUSTDOCFLAGS="--cfg docsrs" cargo +nightly doc --no-deps --all-features --open
