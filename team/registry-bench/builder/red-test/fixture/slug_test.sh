#!/bin/sh
# Red on the fixture as shipped: `slugify` collapses nothing and trims nothing.
set -eu
cat > /tmp/bench-slug-$$.rs <<'RS'
include!("SLUG_PATH");
fn main() {
    assert_eq!(slugify("Hello, World!"), "hello-world");
    assert_eq!(slugify("  spaced  out  "), "spaced-out");
    assert_eq!(slugify("a--b"), "a-b");
    assert_eq!(slugify("Ünïcode 42"), "nicode-42");
    println!("4 cases pass");
}
RS
sed -i.bak "s|SLUG_PATH|$(pwd)/slug.rs|" /tmp/bench-slug-$$.rs
rustc -O -o /tmp/bench-slug-$$ /tmp/bench-slug-$$.rs 2>&1
/tmp/bench-slug-$$
