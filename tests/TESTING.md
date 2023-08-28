# Testing

Per Rust [conventions](https://doc.rust-lang.org/book/ch11-03-test-organization.html), unit tests are kept at the bottom of the main `src/` files in `mod tests`.

As integration tests are added, they will go in `tests/`.

Flat files used by either kind of test can go in `tests/data/`, or `examples/` if they double as user documentation.
